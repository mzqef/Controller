# Distribution Instructions

## Building

1. Install Rust (https://rustup.rs)
2. Clone the repository:
   ```bash
   git clone https://github.com/mzqef/IntelliBoard.git
   cd IntelliBoard
   ```
3. Build in release mode:
   ```bash
   cargo build --release
   ```

## Packaging

- The release binary will be in `target/release/IntelliBoard(.exe)`
- Copy the `config/` directory next to the binary for runtime configuration
- Optionally include `README.md`, `LICENSE`, and example `.env`

## Windows Distribution
- Zip the following files:
  - `target/release/IntelliBoard.exe`
  - `config/`
  - `README.md`, `LICENSE`, `.env.example`

## Linux/macOS Distribution
- Tarball the following files:
  - `target/release/IntelliBoard`
  - `config/`
  - `README.md`, `LICENSE`, `.env.example`

## Example .env
```
API_KEY=sk-your-actual-key-here
```

See `.env.example` in the repository root for a ready-to-copy template.

## License
Distributed under the GPLv3. See LICENSE for details.
