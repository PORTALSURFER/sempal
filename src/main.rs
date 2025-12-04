slint::slint! {
    // Simple hello world window with centered text.
    export component HelloWorld inherits Window {
        width: 320px;
        height: 200px;

        Text {
            text: "Hello, world!";
            horizontal-alignment: center;
            vertical-alignment: center;
        }
    }
}

fn main() -> Result<(), slint::PlatformError> {
    let hello_world = HelloWorld::new()?;
    hello_world.run()?;

    Ok(())
}
