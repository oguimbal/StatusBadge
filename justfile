check:
    cargo fmt --check
    cargo clippy --workspace --all-targets --locked -- -D warnings

test:
    cargo test --workspace --locked
    python3 -m unittest discover -s scripts -p 'test_*.py'

run:
    cargo run -p statusbadge --locked

demo:
    cargo run -p statusbadge --locked -- --demo

bundle:
    ./bundle/bundle.sh

release bump="patch":
    ./release.sh {{bump}}
