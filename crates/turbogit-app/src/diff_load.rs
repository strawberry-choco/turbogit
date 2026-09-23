//! The diff surface's off-frame loads that are *not* cached values.
//!
//! The patch text and the pane bytes both used to be loaded from here by name
//! (`ensure_diff`, `ensure_pane_bytes`), each with the staleness rule written
//! out at its own call site. The patch text now crosses the keyed read
//! ([`crate::keyed_read`]) with it; the pane bytes joined it in ticket 03. What
//! stays is the sourcing an image/binary pane does for itself: the side specs a
//! fetch reads, the decode limits that decide whether a pane shows pixels or
//! the binary caption, and the untracked creation diff the read synthesizes
//! instead of asking git.

use std::path::{Path, PathBuf};

use turbogit_engine_api::GitExecutor;

use crate::events::{DecodedImage, FetchedBlob};

/// A byte size at which an image stops being decoded — beyond it the pane
/// falls back to the binary-change caption, which needs a length, not pixels.
pub const IMAGE_CAP_BYTES: u64 = 20 * 1024 * 1024;

/// A pixel count at which an image stops being decoded, for the same reason:
/// the decode itself would cost more than the frame budget allows.
pub const IMAGE_MAX_PIXELS: u64 = 80_000_000;

/// Where one pane side's bytes come from — mirroring exactly how the viewer's
/// diff text is requested (`PaneTarget`'s side derivation, and the CLI diff):
///
/// | comparison       | old side   | new side     |
/// |------------------|------------|--------------|
/// | Repo (HEAD↔wt)   | `HEAD`     | worktree fs  |
/// | Staged (HEAD↔ix) | `HEAD`     | index `:0`   |
/// | Local (ix↔wt)    | index `:0` | worktree fs  |
/// | explicit l..r    | `<left>`   | `<right>`    |
///
/// Index revs use git's stage syntax (`:<n>:<path>`), the same style the
/// conflict reader uses for `:1`/`:2`/`:3`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SideSpec {
    Rev(String),
    Worktree,
    Missing,
}

/// One side of a non-text pane: the raw bytes behind it, and the repository
/// path they belong to. Owned data because the load runs off the frame path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneSideRequest {
    pub root: PathBuf,
    pub spec: SideSpec,
    /// Repo-relative, slash-separated — the form `git show` wants.
    pub path: String,
}

/// Fetch one side's bytes (worker-thread only): engine blob for rev specs,
/// filesystem for the worktree — metadata-only when no decode is needed,
/// since the binary caption wants lengths, not content.
pub(crate) fn fetch_side(
    exec: &dyn GitExecutor,
    side: &PaneSideRequest,
    decode: bool,
) -> Option<FetchedBlob> {
    let PaneSideRequest { root, spec, path } = side;
    let rel = Path::new(path);
    let bytes = match spec {
        SideSpec::Missing => return None,
        SideSpec::Worktree => {
            let full = root.join(rel);
            if !decode {
                let len = std::fs::metadata(full).ok()?.len();
                return Some(FetchedBlob {
                    byte_len: len,
                    decoded: None,
                });
            }
            std::fs::read(full).ok()?
        }
        SideSpec::Rev(rev) => exec.show_file_bytes(root, rev, rel).ok()?,
    };
    let byte_len = bytes.len() as u64;
    let decoded = decode.then(|| decode_image(&bytes)).flatten();
    Some(FetchedBlob { byte_len, decoded })
}

/// Decode raw bytes into a [`DecodedImage`] when they are an in-cap image.
/// SVG can never reach this — the extension sniff gates decoding — and a blob
/// over [`IMAGE_CAP_BYTES`] or with more than [`IMAGE_MAX_PIXELS`] counts as
/// undecodable so the pane falls back to the binary change.
fn decode_image(bytes: &[u8]) -> Option<DecodedImage> {
    if bytes.len() as u64 > IMAGE_CAP_BYTES {
        return None;
    }
    let img = image::load_from_memory(bytes).ok()?;
    let (width, height) = (img.width(), img.height());
    if width as u64 * height as u64 > IMAGE_MAX_PIXELS {
        return None;
    }
    Some(DecodedImage {
        width,
        height,
        rgba: img.to_rgba8().into_raw(),
    })
}

/// A creation diff for an untracked file, from worktree content, in the exact
/// shape the partial-staging path proves appliable. `None` falls back to engine
/// behaviour (unreadable, binary, or empty files).
pub(crate) fn synthetic_untracked_diff(root: &Path, rel: &Path) -> Option<String> {
    let bytes = std::fs::read(root.join(rel)).ok()?;
    // Binary (NUL byte) or empty content has no meaningful granular diff.
    if bytes.is_empty() || bytes.contains(&0) {
        return None;
    }
    let content = String::from_utf8(bytes).ok()?;
    // Patch headers need slash-separated repo-relative paths.
    let display = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    let n = content.lines().count();
    let mut out = format!(
        "diff --git a/{display} b/{display}\n\
         new file mode 100644\n\
         --- /dev/null\n\
         +++ b/{display}\n\
         @@ -0,0 +1,{n} @@\n"
    );
    for line in content.lines() {
        out.push('+');
        out.push_str(line);
        out.push('\n');
    }
    if !content.ends_with('\n') {
        out.push_str("\\ No newline at end of file\n");
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png() -> Vec<u8> {
        let img = image::DynamicImage::new_rgb8(2, 3);
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        buf.into_inner()
    }

    fn request(root: &Path, spec: SideSpec, path: &str) -> PaneSideRequest {
        PaneSideRequest {
            root: root.to_path_buf(),
            spec,
            path: path.to_owned(),
        }
    }

    /// The caps are the fallback rule: anything the decoder cannot handle
    /// cheaply becomes the binary-change caption rather than a stalled frame.
    #[test]
    fn decoding_gates_on_size_and_pixel_count() {
        assert!(decode_image(b"not an image").is_none());
        assert!(decode_image(&vec![0u8; IMAGE_CAP_BYTES as usize + 1]).is_none());
        assert!(decode_image(&png()).is_some());
    }

    /// A rev side reads through the engine; a worktree side reads the file when
    /// the pane wants pixels and only its length when it wants a caption.
    #[test]
    fn one_side_resolves_its_bytes_from_wherever_it_lives() {
        let png = png();
        let exec = turbogit_engine::fake::FakeExecutor::new();
        exec.files_bytes
            .lock()
            .unwrap()
            .insert(PathBuf::from("art.png"), png.clone());
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("art.png"), &png).unwrap();

        let rev = fetch_side(
            &exec,
            &request(
                Path::new("/irrelevant"),
                SideSpec::Rev("HEAD".to_owned()),
                "art.png",
            ),
            true,
        )
        .expect("the rev side is readable");
        assert_eq!(rev.byte_len, png.len() as u64);
        let decoded = rev.decoded.expect("a png decodes");
        assert_eq!((decoded.width, decoded.height), (2, 3));

        let worktree = fetch_side(
            &exec,
            &request(dir.path(), SideSpec::Worktree, "art.png"),
            true,
        )
        .expect("the worktree side is readable");
        assert_eq!(worktree.decoded.as_ref().map(|d| d.width), Some(2));

        let caption = fetch_side(
            &exec,
            &request(dir.path(), SideSpec::Worktree, "art.png"),
            false,
        )
        .expect("the length alone is enough for a caption");
        assert_eq!(caption.byte_len, png.len() as u64);
        assert!(
            caption.decoded.is_none(),
            "no pixels are decoded when the pane only asks for a size"
        );

        assert!(
            fetch_side(
                &exec,
                &request(dir.path(), SideSpec::Missing, "art.png"),
                true,
            )
            .is_none(),
            "a new or deleted file has one side, not two"
        );
    }

    /// An untracked file is invisible to `git diff`, so its preview is
    /// synthesized in the exact shape the partial-staging path appliates.
    #[test]
    fn an_untracked_preview_is_built_as_a_whole_file_addition() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src/deep")).unwrap();
        std::fs::write(dir.path().join("src/deep/new.rs"), "fn a() {}\n").unwrap();
        let text =
            synthetic_untracked_diff(dir.path(), Path::new("src/deep/new.rs")).expect("built");
        assert!(
            text.starts_with(
                "diff --git a/src/deep/new.rs b/src/deep/new.rs\nnew file mode 100644\n"
            ),
            "patch headers are repo-relative and slash-separated: {text}"
        );
        assert!(text.contains("@@ -0,0 +1,1 @@\n+fn a() {}\n"), "{text}");

        // Empty, binary and absent sides stay the engine's problem.
        std::fs::write(dir.path().join("blob.bin"), [b'a', 0, b'b']).unwrap();
        assert!(synthetic_untracked_diff(dir.path(), Path::new("blob.bin")).is_none());
        std::fs::write(dir.path().join("empty.md"), "").unwrap();
        assert!(synthetic_untracked_diff(dir.path(), Path::new("empty.md")).is_none());
        assert!(synthetic_untracked_diff(dir.path(), Path::new("absent.md")).is_none());
    }

    /// A file without a trailing newline says so, or the last row of the patch
    /// would claim a line break that is not in the content.
    #[test]
    fn a_missing_final_newline_is_disclosed_in_the_patch() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("tight.txt"), "one\ntwo").unwrap();
        let text = synthetic_untracked_diff(dir.path(), Path::new("tight.txt")).expect("built");
        assert!(
            text.ends_with("+two\n\\ No newline at end of file\n"),
            "{text}"
        );
    }
}
