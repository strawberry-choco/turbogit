# Commit window Advanced options is inert chrome in v1

The commit tool window renders the "Advanced options..." disclosure link from
the mockup, but it does not expand in v1. The product spec's advanced-commit
surface (C10: author override, reformat/arrange/optimize pre-checks, etc.) has
no backing implementation, and the redesign's scope is visual.

This is a recorded scope gap under ADR-0016: the link renders because the
mockup shows it; clicking it does nothing until C10 gets a real feature pass.
Rejected: omitting the link (mockup divergence) and expanding to an empty
group (worse than an honest inert control).
