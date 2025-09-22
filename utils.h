#pragma once

#include <assert.h>
#include <stddef.h>
#include <fcntl.h>

#define SERVER_PORT 6741

typedef enum {
    SERVER_MESSAGE_SERVER_ERROR,
    SERVER_MESSAGE_SERVER_FULL
} Server_Message;

const char* server_message_to_cstr(Server_Message msg) {
    switch(msg) {
        case SERVER_MESSAGE_SERVER_ERROR: return "SERVER_MESSAGE_SERVER_ERROR";
        case SERVER_MESSAGE_SERVER_FULL: return "SERVER_MESSAGE_SERVER_FULL";
        default: assert(true && "Unreachable"); return NULL;
    }
}

typedef enum {
    CLIENT_MESSAGE_TYPE_NONE,
    CLIENT_MESSAGE_TYPE_MESSAGE,
    CLIENT_MESSAGE_TYPE_CONNECT
} Client_Message_Type;

typedef struct {
    Client_Message_Type type;
    char message[128];
    size_t message_count;
} Client_Message;

int make_nonblocking(int fd) {
    int flags = fcntl(fd, F_GETFL, 0);
    if(flags == -1) return -1;
    return fcntl(fd, F_SETFL, flags | O_NONBLOCK);
}
