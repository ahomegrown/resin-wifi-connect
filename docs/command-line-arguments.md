# WiFi Connect Command Line Arguments

## Flags

*   **-h, --help**

    Prints help information

*   **-V, --version**

    Prints version information

## Options

Command line options have environment variable counterpart. If both a command line option and its environment variable counterpart are defined, the command line option will take higher precedence.

*   **-d, --portal-dhcp-range** dhcp_range, **$PORTAL_DHCP_RANGE**

    Portal DHCP range

    Default: _192.168.42.2,192.168.42.254_

*   **-g, --portal-gateway** gateway, **$PORTAL_GATEWAY**

    Portal gateway

    Default: _192.168.42.1_

*   **-i, --portal-interface** interface, **$PORTAL_INTERFACE**

    Portal interface

*   **-p, --portal-passphrase** passphrase, **$PORTAL_PASSPHRASE**

    Portal passphrase

    Default:

*   **-s, --portal-ssid** ssid, **$PORTAL_SSID**

    Portal SSID

    Default: _WiFi Connect_

*   **-a, --activity-timeout** timeout, **$ACTIVITY_TIMEOUT**

    Activity timeout (seconds)

    Default: _0 - no timeout_

*   **-u, --ui-directory** ui_directory, **$UI_DIRECTORY**

    Web UI directory location

    Default: _public_
