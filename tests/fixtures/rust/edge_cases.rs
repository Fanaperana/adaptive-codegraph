// RUST EDGE CASES — comprehensive test for tricky patterns

// === 1. Visibility modifiers ===
pub fn public_function() {}
pub(crate) fn crate_visible() {}
pub(super) fn super_visible() {}
fn private_function() {}

// === 2. Async/unsafe/const/extern fn ===
async fn async_handler(data: Vec<u8>) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

unsafe fn raw_pointer_deref(ptr: *const u8) -> u8 {
    *ptr
}

const fn compile_time_calc(n: u32) -> u32 {
    n * 2 + 1
}

extern "C" fn c_abi_function(x: i32) -> i32 {
    x + 1
}

// === 3. Generic structs with lifetime params ===
pub struct RefHolder<'a, T: Clone + Send> {
    data: &'a T,
    count: usize,
    label: &'static str,
}

// === 4. Tuple struct ===
pub struct Wrapper(pub i32);
pub struct Pair<A, B>(A, B);

// === 5. Unit struct ===
pub struct Sentinel;

// === 6. Enum with all variant types ===
#[derive(Debug, Clone)]
pub enum Message {
    Quit,                              // unit variant
    Move { x: i32, y: i32 },          // struct variant
    Write(String),                     // tuple variant
    Color(u8, u8, u8),                // multi-tuple variant
    Nested(Box<Message>),             // recursive variant
}

// === 7. Trait with associated types and default methods ===
pub trait Storage {
    type Key;
    type Value;
    type Error: std::fmt::Debug;

    fn get(&self, key: &Self::Key) -> Result<Option<Self::Value>, Self::Error>;
    fn set(&mut self, key: Self::Key, value: Self::Value) -> Result<(), Self::Error>;

    fn contains(&self, key: &Self::Key) -> bool {
        self.get(key).map(|v| v.is_some()).unwrap_or(false)
    }
}

// === 8. Impl with generics and where clauses ===
impl<'a, T> RefHolder<'a, T>
where
    T: Clone + Send + std::fmt::Debug,
{
    pub fn new(data: &'a T) -> Self {
        Self { data, count: 0, label: "" }
    }

    pub fn with_label(mut self, label: &'static str) -> Self {
        self.label = label;
        self
    }
}

// === 9. Trait impl ===
impl std::fmt::Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

// === 10. Nested modules ===
pub mod outer {
    pub mod inner {
        pub fn deeply_nested() -> u32 { 42 }

        pub struct InnerStruct {
            pub field: String,
        }
    }

    pub fn outer_fn() -> u32 {
        inner::deeply_nested()
    }
}

// === 11. Multiple macros ===
macro_rules! make_getter {
    ($name:ident, $ty:ty, $field:ident) => {
        pub fn $name(&self) -> &$ty {
            &self.$field
        }
    };
}

macro_rules! define_error {
    ($($variant:ident($inner:ty)),+ $(,)?) => {
        #[derive(Debug)]
        pub enum AppError {
            $($variant($inner)),+
        }
    };
}

define_error!(
    Io(std::io::Error),
    Parse(String),
    NotFound(String),
);

// === 12. Complex constants and statics ===
pub const EMPTY_SLICE: &[u8] = &[];
pub const MAGIC_BYTES: [u8; 4] = [0xDE, 0xAD, 0xBE, 0xEF];

pub static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub static mut UNSAFE_GLOBAL: i32 = 0;

// === 13. Type aliases with generics ===
pub type Result<T> = std::result::Result<T, AppError>;
pub type Callback = Box<dyn Fn(i32) -> bool + Send + 'static>;
pub type Map<K, V> = std::collections::HashMap<K, V>;

// === 14. Functions calling across all patterns ===
pub fn integration_test() {
    let _msg = Message::Quit;
    let _w = Wrapper(42);
    let _s = Sentinel;
    let _ = compile_time_calc(10);
    let _ = outer::outer_fn();
    let _ = outer::inner::deeply_nested();
}

// === 15. Nested functions and closures ===
pub fn with_closures() {
    fn helper(x: i32) -> i32 { x + 1 }
    let _add = |a: i32, b: i32| -> i32 { a + b };
    let _result = helper(5);
}

// === 16. Empty trait, empty impl ===
pub trait Marker {}
impl Marker for Wrapper {}

// === 17. Struct with all field types ===
pub struct KitchenSink {
    pub public_field: String,
    private_field: i32,
    pub(crate) crate_field: bool,
    pub generic_field: Option<Vec<String>>,
    pub fn_field: fn(i32) -> bool,
    pub tuple_field: (String, i32, bool),
}
