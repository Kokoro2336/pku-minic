//! Main entry point of the compiler.

use lalrpop_util::lalrpop_mod;
use std::fs::read_to_string;
use std::io::Result;

use iroha::backend::*;
use iroha::frontend::*;
use iroha::opt::*;

#[cfg(feature = "debug")]
use yachiyo::debug::info;
#[cfg(feature = "debug")]
use yachiyo::debug::log::setup;

use yachiyo::cli::Cli;
use yachiyo::debug::DumpASM;
use yachiyo::pass::*;
use yachiyo::utils::arena::Arena;

lalrpop_mod!(sysy);

fn validate_cli(cli: &Cli) {
  if !cli.emit_asm {
    eprintln!("error: missing -S; expected `compiler <input.sysy> -S -o <output.s> [-O1]`");
    std::process::exit(1);
  }

  if cli.output.is_none() {
    eprintln!(
      "error: missing -o <output.s>; expected `compiler <input.sysy> -S -o <output.s> [-O1]`"
    );
    std::process::exit(1);
  }

  if let Some(level) = &cli.opt_level {
    if level != "1" {
      eprintln!("error: unsupported optimization level -O{level}; only -O1 is supported");
      std::process::exit(1);
    }
  }
}

fn main() -> Result<()> {
  // setup logging
  // We need to keep this guard alive for the entire duration of the program.
  #[cfg(feature = "debug")]
  let _guard = setup("rsyc.log");
  #[cfg(feature = "debug")]
  info!("Logger initialized.");

  // Parse the args
  let cli = {
    use clap::Parser;
    Cli::parse()
  };

  validate_cli(&cli);

  let input_path = cli.input.clone();
  // Get input str.
  let input_str = read_to_string(&input_path)?;

  // Parse the input string into an AST.
  let result = {
    let mut parser = Parser::default();
    let root_id = sysy::CompUnitParser::new()
      .parse(&mut parser, &input_str)
      .unwrap();
    // set entry point to the root of the AST
    parser.ast.set_entry(Some(root_id));
    // Clean up the AST.
    parser.ast.gc();
    parser.take()
  };
  // info!("\nParsed result: {:#?}", result);

  let res = std::thread::Builder::new()
    // For now, we set the stack size to 16MB to avoid stack overflow in deep recursion of semantic analysis.
    .stack_size(16 * 1024 * 1024)
    .spawn(move || {
      #[cfg(feature = "debug")]
      info!("Start Semantic Analysis.");

      let result = {
        match Semantic::new(result).run() {
          Ok(res) => res,
          Err(e) => {
            panic!("Semantic Error: {}", e);
          }
        }
      };

      #[cfg(feature = "debug")]
      info!("Finish Semantic Analysis.");
      #[cfg(feature = "debug")]
      info!("Start Emitting.");

      let ir = Emit::new(result).run();

      #[cfg(feature = "debug")]
      info!("Finish Emitting.");

      ir
    })?
    .join();

  let mut ir = match res {
    Ok(ir) => ir,
    Err(e) => {
      panic!("Thread panicked: {:?}", e);
    }
  };

  // Run optimizations.
  PassManager::new(&cli)
    .register(Box::new(Mem2Reg::default()))
    .register(Box::new(RemoveTrivialPhi::default()))
    .register(Box::new(SCCP::default()))
    .register(Box::new(RemoveTrivialPhi::default()))
    .register(Box::new(SimplifyCFG::default()))
    .register(Box::new(RemoveTrivialPhi::default()))
    .register(Box::new(GVN::default()))
    .register(Box::new(DCE::default()))
    .register(Box::new(Compaction::default()))
    .run(&mut ir);

  // Start Lowering
  #[cfg(feature = "debug")]
  info!("Start Lowering.");

  let mut back_ir = Lowering::new(ir).run();

  #[cfg(feature = "debug")]
  info!("Finish Lowering.");

  if cli.dump_asm_after == "Lowering" {
    #[cfg(feature = "debug")]
    info!("Start Dumping Assembly.");

    let asm_filename = cli
      .output
      .as_ref()
      .and_then(|path| path.file_stem())
      .unwrap_or_else(|| std::ffi::OsStr::new("output"))
      .to_string_lossy()
      .to_string();
    DumpASM::new(&back_ir, asm_filename).run();

    #[cfg(feature = "debug")]
    info!("Finish Dumping Assembly.");

    std::process::exit(0);
  }

  // Run Backend Passes.
  BPassManager::new(&cli)
    .register(Box::new(Canonicalize::default()))
    .register(Box::new(ISel::default()))
    .register(Box::new(BDCE::default()))
    .register(Box::new(BCompaction::default()))
    .register(Box::new(RegAlloc::default()))
    .register(Box::new(Peephole::default()))
    .run(&mut back_ir);

  Ok(())
}
