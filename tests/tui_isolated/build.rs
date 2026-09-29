fn main() {
    let path = "__ROOT__/src/db/messages.rs";
    println!("cargo:rerun-if-changed={path}");
    let source = std::fs::read_to_string(path).unwrap();
    let types = source.split("impl Database {").next().unwrap()
        .replace("#[cfg(test)]\n#[path = \"history_budget_tests.rs\"]\nmod history_budget_tests;", "");
    std::fs::write(std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("messages.rs"), types).unwrap();
}
