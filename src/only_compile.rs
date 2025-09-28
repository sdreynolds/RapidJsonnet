use ariadne::Source;
use compiler::Compiler;
use scanner::Scanner;
use std::env;
use std::fs;
use std::fs::File;
use std::io::Write;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    if args.len() == 3 {
        let filename = &args[1];
        let dst_filename = &args[2];

        let source_id: &str = &filename;
        let content = fs::read_to_string(filename)?;

        let source = Source::from(&content);

        // Compile the input
        let mut scanner = Scanner::new(&content, source_id);
        let compiler = Compiler::new(&mut scanner, source_id);
        match compiler.compile() {
            Ok((chunk, _memory_manager)) => {
                println!("✅ Compilation successful!");
                println!(
                    "📊 Generated {} bytes of bytecode with {} constants",
                    chunk.code.len(),
                    chunk.constants.len()
                );

                // Show debug compilation visualization
                let debug_report = chunk.debug_compilation();
                debug_report.print((source_id, &source))?;

                let mut serialized_data = Vec::new();
                // ciborium writes to a writer (like a Vec<u8>)
                ciborium::into_writer(&chunk, &mut serialized_data).expect("Failed to serialize.");
                println!("\nSerialized (ciborium bytes): {:?}", serialized_data);

                let mut file = File::create(dst_filename)?;

                file.write_all(&serialized_data)?;
            }
            Err(compile_error) => {
                println!("❌ Compilation failed:");
                let report = compile_error.into_report();
                report.print((source_id, &source))?;
            }
        }

        Ok(())
    } else {
        Err("Need the two arguments. The first is the source file and the second is the destination file".into())
    }
}
