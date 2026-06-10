//! Thin binary entrypoint (mirrors the lumina server bin's shape: mimalloc
//! global allocator + tokio runtime). The CLI surface and the WS dial loop
//! land in Task 6; until then this stub only proves the runtime + allocator
//! wiring compiles.

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[tokio::main]
async fn main() {
    println!("lumina-companion: connection loop arrives in Task 6 (ADR-0006 Step 1b)");
}
