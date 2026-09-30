fn main() {
    println!("Hello from nuraloumi!");
    println!("{}", greet("World"));
}

fn greet(name: &str) -> String {
    format!("Hello, {}!", name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_greet() {
        assert_eq!(greet("test"), "Hello, test!");
    }
}