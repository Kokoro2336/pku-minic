use lalrpop_util::lalrpop_mod;
use clap::Parser;
use chrono::Local;
use std::path::Path;
use std::fs::read_to_string;
use std::io::Result;
use std::io::Write;

mod ast;
mod config;
mod koopa_ir;
mod util;
use crate::ast::CompUnit;
use crate::koopa_ir::{ast2koopa_ir, Program};
use crate::util::{redirect_stderr, get_abs_path};

// 引用 lalrpop 生成的解析器
// 因为我们刚刚创建了 sysy.lalrpop, 所以模块名是 sysy
lalrpop_mod!(sysy);

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
  /// enable this to transform src to koopa ir.
  #[arg(short='k', long="koopa", default_value_t = false)]
  koopa: bool,

  /// positional argument for input file.
  #[arg(value_name = "INPUT")]
  input: std::path::PathBuf,

  /// use this flag to specify output file.
  #[arg(short, long, default_value = None)]
  output: std::path::PathBuf,
}

fn main() -> Result<()> {
  // preprocess argv so single-dash long-style `-koopa` becomes `--koopa`
  let args = std::env::args_os().enumerate().map(|(i, a)| {
    if i == 0 { return a; }
    if let Some(s) = a.to_str() {
      if s.starts_with('-') && !s.starts_with("--") && &s[1..] == "koopa" {
        return std::ffi::OsString::from(format!("--{}", &s[1..]));
      }
    }
    a
  }).collect::<Vec<_>>();

  let cli = Cli::parse_from(args);

  let input = cli.input;
  let output = cli.output;

  // 读取输入文件
  let input = read_to_string(input)?;

  // 调用 lalrpop 生成的 parser 解析输入文件
  let result = sysy::CompUnitParser::new().parse(&input);
  let ast: CompUnit;
  match result {
    Ok(ast_result) => {
      ast = ast_result;
    }
    Err(e) => {
      panic!("Error during parsing: {:?}", e);
    }
  }

  let koopa_ir: Option<Program> = if cli.koopa {
    // generate Koopa IR
    Some(
      ast2koopa_ir(&ast).unwrap_or_else(|e| {
          eprintln!("Error during AST to Koopa IR transformation: {:?}", e);
          Program::new()
      })
    )
  } else {
    None
  };

  // 输出解析得到的 AST
  println!("{:#?}", ast);
  if let Some(koopa_ir) = koopa_ir {
    let mut f = std::fs::File::create(output)?;
    // output the koopa ir
    f.write_all(format!("{:?}", koopa_ir).as_bytes())?;
  }
  Ok(())
}
