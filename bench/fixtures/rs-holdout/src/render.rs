use crate::shapes::Shape;

pub struct Canvas { // @def Canvas
    pub ops: Vec<String>,
}

impl Canvas {
    pub fn new() -> Self { // @def Canvas::new
        Canvas { ops: Vec::new() }
    }

    pub fn draw(&mut self, s: &dyn Shape) { // @def Canvas::draw
        let d = s.describe();
        self.record(d);
    }

    fn record(&mut self, op: String) { // @def Canvas::record
        self.ops.push(op);
    }
}

impl Default for Canvas {
    fn default() -> Self { // @def Canvas::default
        Self::new()
    }
}
