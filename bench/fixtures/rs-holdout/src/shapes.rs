pub trait Shape { // @def Shape
    fn area(&self) -> f64; // @def Shape::area
    fn describe(&self) -> String { // @def Shape::describe
        format!("area {}", self.area())
    }
}

pub struct Circle { // @def Circle
    pub r: f64,
}

impl Circle {
    pub fn new(r: f64) -> Self { // @def Circle::new
        Self { r }
    }
}

impl Shape for Circle {
    fn area(&self) -> f64 { // @def Circle::area
        std::f64::consts::PI * self.r * self.r
    }
}

pub struct Square { // @def Square
    pub side: f64,
}

impl Shape for Square {
    fn area(&self) -> f64 { // @def Square::area
        self.side * self.side
    }
}
