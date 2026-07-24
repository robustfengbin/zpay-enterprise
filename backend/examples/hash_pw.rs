//! One-shot helper: print the Argon2id PHC hash of the password in env `PW`,
//! using the exact same construction as crate::crypto::password::hash_password
//! (Argon2::default() + OsRng salt) so the output verifies identically.
//! Usage: PW='...' cargo run --example hash_pw

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
    Argon2,
};

fn main() {
    let pw = std::env::var("PW").expect("set PW env var");
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(pw.as_bytes(), &salt)
        .expect("hash");
    println!("{hash}");
}
