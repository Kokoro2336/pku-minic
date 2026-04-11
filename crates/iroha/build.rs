/**
 * Build script for the pku-minic project.
 * */
fn main() {
  lalrpop::Configuration::new()
    .use_cargo_dir_conventions()
    .process_file("src/sysy.lalrpop")
    .unwrap();
}
