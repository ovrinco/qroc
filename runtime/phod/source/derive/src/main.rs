use std::{fs,error::Error};
fn main()->Result<(),Box<dyn Error>> { let path=std::env::args().nth(1).ok_or("canonical JSON required")?;let spec=serde_json::from_slice(&fs::read(path)?)?;let result=phod_local_derive::codegen_rust::generate_rust(&spec)?;fs::write(&result.artifact_filename,&result.artifact_content)?;println!("{}",result.artifact_filename);Ok(()) }
