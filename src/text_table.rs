pub struct TextTableBuilder {
    gap: usize,
    columns: Vec<String>,
}

impl TextTableBuilder {
    fn new() -> Self {
        Self {
            gap: 3,
            columns: vec![],
        }
    }

    pub fn set_gap(mut self, gap: usize) -> Self {
        self.gap = gap;
        self
    }

    pub fn add_column(mut self, column: &str) -> Self {
        self.columns.push(column.into());
        self
    }

    pub fn done(self) -> TextTable {
        let columns = self
            .columns
            .into_iter()
            .map(|column| {
                let len = column.len();
                (column, len)
            })
            .collect();

        TextTable {
            gap: self.gap,
            columns,
            values: vec![],
        }
    }
}

pub struct TextTable {
    gap: usize,
    columns: Vec<(String, usize)>,
    values: Vec<String>,
}

impl TextTable {
    pub fn build() -> TextTableBuilder {
        TextTableBuilder::new()
    }

    pub fn push(&mut self, value: String) {
        let i = self.values.len() % self.columns.len();
        let col = &mut self.columns[i].1;
        *col = usize::max(*col, value.len());
        self.values.push(value);
    }

    pub fn print(&self) {
        for (i, (column, max_width)) in self.columns.iter().enumerate() {
            let rpad = " ".repeat(*max_width - column.len());
            print!("{}{}", column.to_uppercase(), rpad);
            if i < self.columns.len() - 1 {
                print!("{}", " ".repeat(self.gap));
            } else {
                print!("\n");
            }
        }

        for (i, value) in self.values.iter().enumerate() {
            let j = i % self.columns.len();
            let max_width = self.columns[j].1;
            let rpad = " ".repeat(max_width - value.len());
            print!("{}{}", value, rpad);
            if j < self.columns.len() - 1 {
                print!("{}", " ".repeat(self.gap));
            } else {
                print!("\n");
            }
        }
    }
}
