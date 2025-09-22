#include <stdio.h>
#include <stdlib.h>
#include <stdbool.h>
#include <string.h>
#include <unistd.h>
#include <errno.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <sys/epoll.h>
#include <arpa/inet.h>
#include <pthread.h>

#include "utils.h"

void* receive_messages(void* args) {
    int client_fd = *(int*)args;

    while(true) {
        Client_Message other_client_msg;
        ssize_t received = recv(client_fd, &other_client_msg, sizeof(other_client_msg), MSG_DONTWAIT);
        if(received > 0) printf("%s\n", other_client_msg.message);
        else if(received == 0) {
            printf("Connection closed by server\n");
            close(client_fd);
            break;
        }
        sleep(1);
    }

    return NULL;
}

typedef struct {
    int client_fd;
    struct sockaddr_in server_addr;
} Client_Context;

bool client_context_create(Client_Context* ctx) {
    ctx->client_fd = socket(AF_INET, SOCK_STREAM, 0);
    if(ctx->client_fd == -1) {
        fprintf(stderr, "Failed to create client_fd: %s\n", strerror(errno));
        return false;
    }

    int opt = 1;
    if(setsockopt(ctx->client_fd, SOL_SOCKET, SO_REUSEADDR, &opt, sizeof(opt)) == -1) {
        fprintf(stderr, "Failed to setsockopt: %s\n", strerror(errno));
        close(ctx->client_fd);
        return false;
    }

    return true;
}

bool client_connect(Client_Context* ctx, const char* server_address, int server_port) {
    ctx->server_addr.sin_family = AF_INET;
    ctx->server_addr.sin_port = htons(server_port);
    if(inet_pton(AF_INET, server_address, &ctx->server_addr.sin_addr) <= 0) {
        fprintf(stderr, "The address '%s' is invalid\n", server_address);
        close(ctx->client_fd);
        return false;
    }

    printf("Connecting...\n");
    if(connect(ctx->client_fd, (struct sockaddr*)&ctx->server_addr, sizeof(ctx->server_addr)) == -1) {
        fprintf(stderr, "Failed to connect: %s\n", strerror(errno));
        close(ctx->client_fd);
        return false;
    }

    if(make_nonblocking(ctx->client_fd) == -1) {
        fprintf(stderr, "Failed to make client_fd non-blocking: %s\n", strerror(errno));
        close(ctx->client_fd);
        return false;
    }

    Server_Message server_msg;
    ssize_t received = recv(ctx->client_fd, &server_msg, sizeof(server_msg), MSG_DONTWAIT);
    if(received > 0) {
        fprintf(stderr, "Failed to connect due to server error: %s\n", server_message_to_cstr(server_msg));
        close(ctx->client_fd);
        return false;
    } else if(received == 0) {
        printf("Connection closed by server\n");
        close(ctx->client_fd);
        return false;
    } else {
        if(errno != EAGAIN && errno != EWOULDBLOCK) {
            fprintf(stderr, "recv error: %s\n", strerror(errno));
            close(ctx->client_fd);
            return false;
        }
    }

    printf("Connected\n");
    printf("Enter your name (max 20 characters): ");
    char name[21];
    fgets(name, sizeof(name), stdin);
    name[strcspn(name, "\n")] = '\0';

    Client_Message msg = {0};
    msg.type = CLIENT_MESSAGE_TYPE_CONNECT;
    strncpy(msg.message, name, sizeof(msg.message));
    msg.message_count = strlen(name);

    if(write(ctx->client_fd, &msg, sizeof(msg)) == -1 && errno != EAGAIN) {
        fprintf(stderr, "Failed to send connect message: %s\n", strerror(errno));
        close(ctx->client_fd);
        return false;
    }

    printf("Welcome %s! To send a message just type the message and hit enter.\n", name);

    return true;
}

int main(void) {
    printf("Enter the server address: ");
    char server_address[16];
    fgets(server_address, sizeof(server_address), stdin);
    server_address[strcspn(server_address, "\n")] = '\0';

    Client_Context ctx = {0};
    if(!client_context_create(&ctx)) return 1;
    if(!client_connect(&ctx, server_address, SERVER_PORT)) return 1;

    pthread_t th_receive_messages;
    if(pthread_create(&th_receive_messages, NULL, receive_messages, &ctx.client_fd) == EAGAIN) {
        fprintf(stderr, "Are you serious? You dont have enough resources for 2 threads!?\n");
        close(ctx.client_fd);
        return 1;
    }

    while(true) {
        Client_Message client_msg = {0};

        char message[128];
        fgets(message, sizeof(message), stdin);
        message[strcspn(message, "\n")] = '\0';

        client_msg.type = CLIENT_MESSAGE_TYPE_MESSAGE;
        strncpy(client_msg.message, message, sizeof(client_msg.message));
        client_msg.message_count = strlen(message);

        write(ctx.client_fd, &client_msg, sizeof(client_msg));
    }

    return 0;
}
