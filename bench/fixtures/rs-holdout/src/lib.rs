pub mod render;
pub mod shapes;

use render::Canvas;
use shapes::{Circle, Shape, Square};

pub fn draw_all(canvas: &mut Canvas) -> usize { // @def draw_all
    let shapes: Vec<Box<dyn Shape>> = vec![Box::new(Circle::new(1.0)), Box::new(Square { side: 2.0 })];
    for s in &shapes {
        canvas.draw(s.as_ref());
    }
    shapes.len()
}

pub fn total_area(items: &[Circle]) -> f64 { // @def total_area
    items.iter().map(|c| c.area()).sum()
}
