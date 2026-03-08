#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModuleDemand {
    pub needs_receipt_graph: bool,
    pub needs_raw_projection: bool,
    pub needs_hos_delta: bool,
}

impl ModuleDemand {
    pub fn union(self, other: Self) -> Self {
        Self {
            needs_receipt_graph: self.needs_receipt_graph || other.needs_receipt_graph,
            needs_raw_projection: self.needs_raw_projection || other.needs_raw_projection,
            needs_hos_delta: self.needs_hos_delta || other.needs_hos_delta,
        }
    }
}
