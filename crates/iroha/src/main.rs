use lalrpop_util::lalrpop_mod;
use std::fs::read_to_string;
use std::io::Result;

use iroha::backend::*;
use iroha::frontend::*;
use iroha::opt::*;

use yachiyo::cli::Cli;
use yachiyo::debug::info;
use yachiyo::debug::log::setup;
use yachiyo::pass::*;
use yachiyo::utils::arena::Arena;

lalrpop_mod!(sysy);

fn main() -> Result<()> {
    // setup logging
    // We need to keep this guard alive for the entire duration of the program.
    let _guard = setup("rsyc.log");
    info!("Logger initialized.");

    // Parse the args
    let cli = {
        use clap::Parser;
        Cli::parse()
    };

    let input_path = cli.input.clone();
    let _ = cli.output.clone();

    // Get input str.
    let input_str = read_to_string(&input_path)?;

    // Parse the input string into an AST.
    let result = {
        let mut parser = Parser::default();
        let root_id = sysy::CompUnitParser::new()
            .parse(&mut parser, &input_str)
            .unwrap();
        // set entry point to the root of the AST
        parser.ast.set_entry(root_id);
        // Clean up the AST.
        parser.ast.gc();
        parser.take()
    };
    // info!("\nParsed result: {:#?}", result);

    let res = std::thread::Builder::new()
        // For now, we set the stack size to 16MB to avoid stack overflow in deep recursion of semantic analysis.
        .stack_size(16 * 1024 * 1024)
        .spawn(move || {
            info!("Start Semantic Analysis.");
            let result = {
                match Semantic::new(result).run() {
                    Ok(res) => res,
                    Err(e) => {
                        panic!("Semantic Error: {}", e);
                    }
                }
            };
            info!("Finish Semantic Analysis.");

            info!("Start Emitting.");
            let ir = Emit::new(result).run();
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
        .register(Box::new(DCE::default()))
        .register(Box::new(Compaction::default()))
        .run(&mut ir);

    // Start Lowering
    info!("Start Lowering.");
    let mut back_ir = Lowering::new(ir).run();
    info!("Finish Lowering.");

    // Run Backend Passes.
    BPassManager::default()
        .register(Box::new(ISel::default()))
        .register(Box::new(RegAlloc::default()))
        .run(&mut back_ir);

    // Dump the asm.

    Ok(())
}
