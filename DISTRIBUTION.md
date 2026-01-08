# Distribution Instructions

## Building

1. Install Rust (https://rustup.rs)
2. Clone the repository:
   ```bash
   git clone https://github.com/your-org/controller.git
   cd controller
   ```
3. Build in release mode:
   ```bash
   cargo build --release
   ```

## Packaging

- The release binary will be in `target/release/Controller(.exe)`
- Copy the `config/` directory next to the binary for runtime configuration
- Optionally include `README.md`, `LICENSE`, and example `.env`

## Windows Distribution
- Zip the following files:
  - `target/release/Controller.exe`
  - `config/`
  - `README.md`, `LICENSE`, `.env.example`

## Linux/macOS Distribution
- Tarball the following files:
  - `target/release/Controller`
  - `config/`
  - `README.md`, `LICENSE`, `.env.example`

## Example .env
```
ALIYUN_API_KEY=sk-your-actual-key-here
```

## License
Distributed under the GPLv3. See LICENSE for details.
