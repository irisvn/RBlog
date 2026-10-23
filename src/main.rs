fn main() -> std::process::ExitCode {
    rustcms::Console::<rblog::database::migrations::Migrator>::new(rblog::bootstrap::build).run_blocking()
}
