OpenSSH compatibility environment for `tests/openssh_compat.rs`.

`test_ed25519` is a throwaway TEST-ONLY key with no access to anything but these
containers. `Cargo.toml` excludes this directory from the packaged crate.

    docker compose -f crates/pylentra-core/tests/compat/docker-compose.yml up -d --build
    PYLENTRA_COMPAT=1 cargo test -p pylentra-core --test openssh_compat -- --ignored
    docker compose -f crates/pylentra-core/tests/compat/docker-compose.yml down
