//! CLI support.

use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
  /// enable this to stop running optimization passes and directly emit LLVM IR.
  #[arg(long = "emit-llvm", default_value_t = false)]
  pub emit_llvm: bool,

  #[arg(long = "dump-llvm-after", default_value_t = String::new())]
  pub dump_llvm_after: String,

  #[arg(long = "dump-asm-after", default_value_t = String::new())]
  pub dump_asm_after: String,

  /// emit assembly for -o output.
  #[arg(short = 'S', default_value_t = false)]
  pub emit_asm: bool,

  /// optimization level; the contest interface supports -O1.
  #[arg(short = 'O', value_name = "LEVEL")]
  pub opt_level: Option<String>,

  /// positional argument for input file.
  #[arg(value_name = "INPUT")]
  pub input: std::path::PathBuf,

  /// use this flag to specify output file.
  #[arg(short, long)]
  pub output: Option<std::path::PathBuf>,
}
