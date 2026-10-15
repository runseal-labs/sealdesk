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
    puts("PASS tailcat C ABI: QR allocation/free and idle server shutdown");
    return 0;
}
