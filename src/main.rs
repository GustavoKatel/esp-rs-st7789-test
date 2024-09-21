use anyhow::anyhow;
use display_interface_spi::SPIInterface;
use embedded_graphics::{pixelcolor::Rgb565, prelude::*}; // Provides the required color type
use esp_idf_svc::{
    hal::{
        delay::Ets,
        gpio::{AnyIOPin, PinDriver},
        prelude::Peripherals,
        spi::{
            config::{Config as SPIConfig, DriverConfig, MODE_0, MODE_3},
            Dma, SpiDeviceDriver, SpiDriver, SPI2,
        },
        units::FromValueType,
    },
    log::EspLogger,
};
use log::{error, info};
use mipidsi::{models, options, Builder};
use tokio::time::{sleep, Duration};

fn main() -> anyhow::Result<()> {
    // It is necessary to call this function once. Otherwise some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_svc::sys::link_patches();

    EspLogger::initialize_default();

    // `tokio` uses the ESP IDF `eventfd` syscall to implement async IO.
    esp_idf_svc::io::vfs::initialize_eventfd(5)?;

    // This thread is necessary because the ESP IDF main task thread is running with a very low priority that cannot be raised
    // (lower than the hidden posix thread in `async-io`)
    // As a result, the main thread is constantly starving because of the higher prio `async-io` thread
    //
    // To use async networking IO, make your `main()` minimal by just spawning all work in a new thread
    std::thread::Builder::new()
        .stack_size(60000)
        .spawn(run_main)
        .unwrap()
        .join()
        .unwrap()
        .unwrap();

    Ok(())
}

fn run_main() -> anyhow::Result<()> {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let res = match init().await {
            Ok(_) => {
                info!("finished!");
                Ok(())
            }
            Err(e) => {
                error!("failed: {:?}", e);
                Err(e)
            }
        };

        sleep(Duration::from_secs(5)).await;

        res
    })?;

    Ok(())
}

async fn init() -> anyhow::Result<()> {
    info!("Initializing services...");

    let peripherals =
        Peripherals::take().map_err(|err| anyhow!("Failed to take peripherals: {:?}", err))?;

    let mut rst = PinDriver::output(peripherals.pins.gpio0)?;
    rst.set_high()?;

    let dc = PinDriver::output(peripherals.pins.gpio1)?;
    let mut delay = Ets;

    let mut backlight = PinDriver::output(peripherals.pins.gpio10)?;
    backlight.set_high()?;

    let scl = peripherals.pins.gpio2;
    let spi = peripherals.spi2;
    let sda = peripherals.pins.gpio4;

    let spi = SpiDriver::new(
        spi,
        scl,
        sda,
        None::<AnyIOPin>,
        &DriverConfig {
            // dma: Dma::Auto(240 * 240 * 2 + 8),
            ..Default::default()
        },
    )?;

    let cs: Option<AnyIOPin> = None;

    let spi = SpiDeviceDriver::new(
        spi,
        cs,
        &SPIConfig {
            baudrate: 26.MHz().into(),
            data_mode: MODE_3,
            ..Default::default()
        },
    )?;

    let di = SPIInterface::new(spi, dc);
    let mut display = Builder::new(models::ST7789, di)
        .reset_pin(rst)
        .display_size(240, 240)
        .init(&mut delay)
        .map_err(|err| anyhow!("Error initializing display: {:?}", err))?;

    // Clear the display to red
    info!("Clearing display...");
    display
        .clear(Rgb565::RED)
        .map_err(|err| anyhow!("Error clearing display: {:?}", err))?;
    info!("Display cleared!");

    info!("Displaying image...");
    // Clear the display initially
    display.clear(Rgb565::GREEN).unwrap();

    // Draw a rectangle on screen
    let style = embedded_graphics::primitives::PrimitiveStyleBuilder::new()
        .fill_color(Rgb565::GREEN)
        .build();

    embedded_graphics::primitives::Rectangle::new(Point::zero(), display.bounding_box().size)
        .into_styled(style)
        .draw(&mut display)
        .unwrap();
    info!("Displaying image... DONE");

    loop {
        sleep(Duration::from_secs(1)).await;
    }
}
