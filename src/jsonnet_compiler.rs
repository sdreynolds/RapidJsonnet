// Copyright 2026 Scott Reynolds
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

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
