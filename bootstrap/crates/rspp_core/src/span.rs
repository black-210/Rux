#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub col: usize,
    pub end_line: usize,
    pub end_col: usize,
}

impl Span {
    pub fn new(start: usize, end: usize, line: usize, col: usize, end_line: usize, end_col: usize) -> Self {
        Self { start, end, line, col, end_line, end_col }
    }

    pub fn point(offset: usize, line: usize, col: usize) -> Self {
        Self {
            start: offset,
            end: offset + 1,
            line,
            col,
            end_line: line,
            end_col: col + 1,
        }
    }

    pub fn merge(self, other: Span) -> Span {
        if self.start <= other.start {
            Span {
                start: self.start,
                end: other.end.max(self.end),
                line: self.line,
                col: self.col,
                end_line: other.end_line.max(self.end_line),
                end_col: if other.end_line >= self.end_line { other.end_col } else { self.end_col },
            }
        } else {
            other.merge(self)
        }
    }
}
