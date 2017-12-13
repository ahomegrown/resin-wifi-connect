#!/usr/bin/env bash

export DBUS_SYSTEM_BUS_ADDRESS=unix:path=/host/run/dbus/system_bus_socket

# Choose a condition for running WiFi Connect according to your use case:

# 1. Is there Internet connectivity?
# nmcli -t g | grep full

# 2. Is there a default gateway?
# ip route | grep default

# 3. Is there an active connection?
# nmcli -t c show --active | grep :

# 4. Is there an active WiFi connection?
nmcli -t c show --active | grep 802-11-wireless

if [ $? -eq 0 ]; then
    printf 'Skipping WiFi Connect'
else
    printf 'Starting WiFi Connect'
    ./resin-wifi-connect
fi

# Start your application here.
