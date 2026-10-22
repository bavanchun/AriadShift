fn main() {
    // New migration files must invalidate sqlx's embedded migration list too.
    println!("cargo:rerun-if-changed=migrations");
}
