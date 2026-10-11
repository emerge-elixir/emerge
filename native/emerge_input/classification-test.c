/* MIT. Exercises the actual upstream capability classifier, without sysfs or udev. */
int emerge_input_classify(const char *ev, const char *abs, const char *rel,
                         const char *keys, const char *props) {
    struct udev_device device = {0};
    udev_list_entry_init(&device.properties);
    udev_list_entry_add(&device.properties, "SUBSYSTEM", "input", 0);
    udev_list_entry_add(&device.properties, "EV", ev, 0);
    udev_list_entry_add(&device.properties, "ABS", abs, 0);
    udev_list_entry_add(&device.properties, "REL", rel, 0);
    udev_list_entry_add(&device.properties, "KEY", keys, 0);
    udev_list_entry_add(&device.properties, "PROP", props, 0);
    set_properties_from_evdev(&device);
    const char *types[] = {"ID_INPUT_MOUSE", "ID_INPUT_KEYBOARD", "ID_INPUT_TOUCHSCREEN",
                          "ID_INPUT_TOUCHPAD", "ID_INPUT_TABLET", "ID_INPUT_ACCELEROMETER",
                          "ID_INPUT_JOYSTICK", "ID_INPUT_SWITCH", "ID_INPUT_KEY"};
    int result = 0;
    for (unsigned i = 0; i < sizeof(types) / sizeof(types[0]); i++)
        if (udev_device_get_property_value(&device, types[i])) result |= 1 << i;
    udev_list_entry_free_all(&device.properties);
    return result;
}
