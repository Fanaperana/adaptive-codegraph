pub fn checksum(s: &str) -> u32 { // @def util::checksum
    s.bytes().map(|b| b as u32).sum()
}

pub fn helper() { // @def util::helper
    let n = checksum("h");
    println!("{n}");
}
