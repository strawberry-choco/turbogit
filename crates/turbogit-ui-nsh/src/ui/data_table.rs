//! The data table over nsh-base's DataTable: virtualizing rows, sortable
//! and resizable columns, row height from the density tokens. The parts the
//! app styles name the chrome tokens through Tailwind classes.

use nsh_base::{
    DataTableCell, DataTableClasses, DataTableColumn, DataTableRows, data_table as base_table,
};
use nsh_core::prelude::*;

use crate::theme as data;

/// One column of a table.
#[derive(Debug, Clone)]
pub struct DataTableColumnProps {
    /// The field of the row data the column reads.
    pub key: SharedString,
    /// The header text.
    pub title: SharedString,
    /// The column's width, when fixed.
    pub width: Option<f32>,
}

impl DataTableColumnProps {
    pub fn new(key: impl Into<SharedString>, title: impl Into<SharedString>) -> Self {
        DataTableColumnProps {
            key: key.into(),
            title: title.into(),
            width: None,
        }
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    fn into_column(self) -> DataTableColumn {
        let mut column = DataTableColumn::default();
        column.key = self.key;
        column.title = Some(self.title);
        if let Some(width) = self.width {
            column.width = Some(px(width));
        }
        column
    }
}

/// A cell the app draws.
pub type CellPaint = std::rc::Rc<dyn Fn(&DataTableCell) -> AnyElement>;

/// The props of [`data_table`].
pub struct DataTableProps {
    pub columns: Vec<DataTableColumnProps>,
    /// The rows: columns of cells by field name.
    pub data: DataTableRows,
    /// Row height, the file row height by default.
    pub row_height: f32,
    /// What screen readers call the table.
    pub accessibility_label: Option<SharedString>,
    pub test_id: Option<SharedString>,
    /// A cell the app draws: a row action, a state dot.
    pub render_cell: Option<CellPaint>,
}

impl DataTableProps {
    pub fn new(columns: Vec<DataTableColumnProps>, data: DataTableRows) -> Self {
        DataTableProps {
            columns,
            data,
            row_height: data::FILE_ROW_HEIGHT,
            accessibility_label: None,
            test_id: None,
            render_cell: None,
        }
    }

    pub fn render_cell(mut self, render: impl Fn(&DataTableCell) -> AnyElement + 'static) -> Self {
        self.render_cell = Some(std::rc::Rc::new(render));
        self
    }

    pub fn row_height(mut self, row_height: f32) -> Self {
        self.row_height = row_height;
        self
    }

    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessibility_label = Some(label.into());
        self
    }

    pub fn test_id(mut self, test_id: &str) -> Self {
        self.test_id = Some(SharedString::from(test_id.to_string()));
        self
    }
}

/// A data table: the header row's muted title ink, the row height the
/// density tokens set. The table element takes no test id, so the wrapper
/// names the view that carries it.
pub fn data_table(props: DataTableProps) -> impl IntoElement {
    let columns: Vec<DataTableColumn> = props
        .columns
        .into_iter()
        .map(DataTableColumnProps::into_column)
        .collect();
    // Non-exhaustive in 0.5.x: build via Default, then name each part.
    let mut classes = DataTableClasses::default();
    classes.root = Some("rounded-md bg-background".into());
    classes.header_cell = Some("text-xs text-muted-foreground".into());
    classes.row = Some("text-foreground".into());
    classes.row_hover = Some("bg-muted".into());
    classes.row_selected = Some("bg-accent".into());
    let inner = base_table()
        .columns(columns)
        .data(props.data)
        .row_height(px(props.row_height))
        .classes(classes);
    let inner = match props.render_cell.clone() {
        Some(render) => inner.render_cell(move |cell: &DataTableCell| render(cell)),
        None => inner,
    };
    let inner = inner.w_full().h_full();
    let mut host = view().flex_1().w_full();
    if let Some(label) = props.accessibility_label.clone() {
        host = host.label(label);
    }
    let host = host.child(inner);
    match props.test_id.clone() {
        Some(id) => view().flex_1().w_full().test_id(&id).child(host),
        None => view().flex_1().w_full().child(host),
    }
}
