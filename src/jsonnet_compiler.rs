use compiler::Compiler;
use memory_manager::MemoryManager;
use scanner::Scanner;
use std::env;
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    let filename = args
        .get(1)
        .expect("Usage: jsonnet_compiler <input> [<output>]");

    let output_path = match args.get(2) {
        Some(path) => path.clone(),
        None => format!("{}c", filename),
    };

    let content = fs::read_to_string(filename)?;

    let mut scanner = Scanner::new(&content, filename);
    let mut memory_manager = MemoryManager::new();
    let compiler = Compiler::new(&mut scanner, filename);
    let chunk = compiler
        .compile(&mut memory_manager)
        .map_err(|e| format!("Compilation failed: {:?}", e))?;

    let bytes = serialized_chunk::serialize_program(&chunk, &memory_manager);

    fs::write(&output_path, &bytes)?;

    println!(
        "Compiled {} -> {} ({} bytes)",
        filename,
        output_path,
        bytes.len()
    );

    Ok(())
}
