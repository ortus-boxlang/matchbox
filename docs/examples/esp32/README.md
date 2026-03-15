# ESP32 BoxLang Example

This example demonstrates how to compile and flash a simple BoxLang script to an ESP32 microcontroller.

## Running the Example

1.  Connect your ESP32 device to your computer via USB.
2.  Identify your chip type (e.g., `esp32s3`, `esp32c3`, or standard `esp32`).
3.  Run the following command (replacing `esp32s3` with your chip type):

```bash
matchbox docs/examples/esp32/hello_esp32.bxs --target esp32 --chip esp32s3 --full-flash
```

## What Happens Next?

1.  MatchBox will compile the BoxLang script into bytecode.
2.  It will then compile a custom ESP32 firmware binary that includes the MatchBox VM and your script.
3.  `espflash` will be invoked to send the binary to your device.
4.  Once flashed, you can monitor the serial output:
    ```bash
    espflash monitor
    ```

You should see the "Hello from ESP32!" message and the loop iterations in your terminal.
