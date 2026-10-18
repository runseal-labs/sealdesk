#include "libtailcat.h"
#include <stdio.h>
#include <string.h>

int main(void) {
    char *response = TcStopServe();
    if (response == NULL || response[0] != '\0') {
        TcFree(response);
        return 1;
    }
    TcFree(response);

    response = TcQR("SealDesk archive smoke");
    if (response == NULL || response[0] == '\0' || strncmp(response, "ERR:", 4) == 0) {
        TcFree(response);
        return 2;
    }
    TcFree(response);
    response = TcServeWithDERPMap("", "", 0, "", "http://invalid.example/map.json");
    if (response == NULL || strncmp(response, "ERR:DERP map configuration:", 27) != 0) {
        TcFree(response);
        return 3;
    }
    TcFree(response);
    response = TcForwardWithDERPMap("", "", 1000, "", "http://invalid.example/map.json");
    if (response == NULL || strncmp(response, "ERR:DERP map configuration:", 27) != 0) {
        TcFree(response);
        return 4;
    }
    TcFree(response);
    puts("PASS tailcat C ABI: QR allocation/free, idle shutdown, and configured host/client validation");
    return 0;
}
