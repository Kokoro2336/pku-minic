//! Build script of the compiler.

fn main() {
  lalrpop::Configuration::new()
    .use_cargo_dir_conventions()
    .process_file("src/sysy.lalrpop")
    .unwrap();
}
