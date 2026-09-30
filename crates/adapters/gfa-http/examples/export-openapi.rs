fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", gfa_http::openapi_document(true).to_pretty_json()?);
    Ok(())
}
