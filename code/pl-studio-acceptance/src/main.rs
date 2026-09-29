fn main() -> anyhow::Result<()> {
    use clap::Parser;

    let options = manual_gui::ManualGuiOptions::parse();
    manual_gui::run(options)
}

mod manual_gui;
