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

#include "utils.h"

#define MAX_EVENTS 64
#define MAX_CLIENTS 10

typedef struct {
    int fd;
    char name[21];
} Client;

typedef struct {
    int server_fd;
    struct sockaddr_in server_addr;
    int epoll_fd;
    struct epoll_event event;
    struct epoll_event events[MAX_EVENTS];
    Client clients[MAX_CLIENTS];
    size_t client_count;
} Server_Context;

bool server_context_create(Server_Context* ctx) {
    ctx->server_fd = socket(AF_INET, SOCK_STREAM, 0);
    if(ctx->server_fd == -1) {
        fprintf(stderr, "Failed to create server_fd: %s\n", strerror(errno));
        return false;
    }

    int opt = 1;
    if(setsockopt(ctx->server_fd, SOL_SOCKET, SO_REUSEADDR, &opt, sizeof(opt)) == -1) {
        fprintf(stderr, "Failed to setsockopt: %s\n", strerror(errno));
        close(ctx->server_fd);
        return false;
    }

    return true;
}

bool server_listen(Server_Context* ctx, const char* addr, int port) {
    ctx->server_addr.sin_family = AF_INET;
    ctx->server_addr.sin_port = htons(port);
    if(!inet_aton(addr, &ctx->server_addr.sin_addr)) {
        fprintf(stderr, "Invalid string address '%s'\n", addr);
        close(ctx->server_fd);
        return false;
    }

    if(bind(ctx->server_fd, (struct sockaddr*)&ctx->server_addr, sizeof(ctx->server_addr)) == -1) {
        fprintf(stderr, "Failed to bind: %s\n", strerror(errno));
        close(ctx->server_fd);
        return false;
    }

    if(listen(ctx->server_fd, SOMAXCONN) == -1) {
        fprintf(stderr, "Failed to listen: %s\n", strerror(errno));
        close(ctx->server_fd);
        return false;
    }

    if(make_nonblocking(ctx->server_fd) == -1) {
        fprintf(stderr, "Failed to make listening socket non-blocking: %s\n", strerror(errno));
        close(ctx->server_fd);
        return false;
    }

    ctx->epoll_fd = epoll_create1(0);
    if(ctx->epoll_fd == -1) {
        fprintf(stderr, "Failed to create epoll instance: %s\n", strerror(errno));
        close(ctx->server_fd);
        return false;
    }

    ctx->event.events = EPOLLIN;
    ctx->event.data.fd = ctx->server_fd;
    if(epoll_ctl(ctx->epoll_fd, EPOLL_CTL_ADD, ctx->server_fd, &ctx->event) == -1) {
        fprintf(stderr, "Failed to epoll_ctl: %s\n", strerror(errno));
        close(ctx->epoll_fd);
        close(ctx->server_fd);
        return false;
    }

    printf("Server listening on port %d...\n", port);

    return true;
}

const char* server_get_client_name(Server_Context* ctx, int fd) {
    for(size_t i = 0; i < ctx->client_count; ++i) {
        if(ctx->clients[i].fd == fd) return ctx->clients[i].name;
    }
    return "<unknown>";
}

void server_close_client_fd(int client_fd, Server_Message msg) {
    write(client_fd, &msg, sizeof(msg));
    close(client_fd);
}

void server_delete_client(Server_Context* ctx, int client_fd) {
    epoll_ctl(ctx->epoll_fd, EPOLL_CTL_DEL, client_fd, NULL);
    close(client_fd);

    for(size_t i = 0; i < ctx->client_count; ++i) {
        if(ctx->clients[i].fd == client_fd) {
            ctx->clients[i] = ctx->clients[--ctx->client_count];
            break;
        }
    }
}

bool server_handle_events(Server_Context* ctx) {
    int n_fd = epoll_wait(ctx->epoll_fd, ctx->events, MAX_EVENTS, -1);
    if(n_fd == -1) {
        if(errno == EINTR) return true;
        fprintf(stderr, "Failed to epoll_wait: %s\n", strerror(errno));
        close(ctx->epoll_fd);
        close(ctx->server_fd);
        return false;
    }

    for(int i = 0; i < n_fd; ++i) {
        int fd = ctx->events[i].data.fd;
        if(fd == ctx->server_fd) {
            while(true) {
                int client_fd = accept(ctx->server_fd, NULL, NULL);
                if(client_fd == -1) {
                    if(errno == EAGAIN || errno == EWOULDBLOCK) break;
                    fprintf(stderr, "Failed to accept client: %s\n", strerror(errno));
                    continue;
                }

                printf("Incoming connection: fd=%d\n", client_fd);

                if(make_nonblocking(client_fd) == -1) {
                    fprintf(stderr, "Failed to make new client socket non-blocking: %s\n", strerror(errno));
                    server_close_client_fd(client_fd, SERVER_MESSAGE_SERVER_ERROR);
                    continue;
                }

                if(ctx->client_count < MAX_CLIENTS) {
                    ctx->event.events = EPOLLIN | EPOLLET;
                    ctx->event.data.fd = client_fd;
                    if(epoll_ctl(ctx->epoll_fd, EPOLL_CTL_ADD, client_fd, &ctx->event) == -1) {
                        fprintf(stderr, "Failed to add a new client to epoll list: %s\n", strerror(errno));
                        server_close_client_fd(client_fd, SERVER_MESSAGE_SERVER_ERROR);
                        continue;
                    }

                    ctx->clients[ctx->client_count++].fd = client_fd;
                    ctx->clients[ctx->client_count - 1].name[0] = '\0';
                } else {
                    printf("Cant add a new client because the server is full.\n");
                    server_close_client_fd(client_fd, SERVER_MESSAGE_SERVER_ERROR);
                }
            }
        } else {
            while(true) {
                Client_Message client_msg = {0};
                ssize_t count = read(fd, &client_msg, sizeof(client_msg));
                if(count == -1) {
                    if(errno == EAGAIN || errno == EWOULDBLOCK) break;

                    fprintf(stderr, "Failed to read from client '%s': %s\n", server_get_client_name(ctx, fd), strerror(errno));
                    server_delete_client(ctx, fd);
                    break;
                } else if(count == 0) {
                    printf("Client fd=%d '%s' disconnected\n", fd, server_get_client_name(ctx, fd));
                    server_delete_client(ctx, fd);
                    break;
                }

                size_t idx = client_msg.message_count;
                if(idx >= sizeof(client_msg.message)) idx = sizeof(client_msg.message) - 1;
                client_msg.message[idx] = '\0';

                if(client_msg.type == CLIENT_MESSAGE_TYPE_MESSAGE) {
                    const char* sender_name = server_get_client_name(ctx, fd);
                    printf("%s says: %s\n", sender_name, client_msg.message);

                    for(size_t j = 0; j < ctx->client_count; ++j) {
                        if(ctx->clients[j].fd != fd) {
                            Client_Message out_msg = {0};
                            out_msg.type = CLIENT_MESSAGE_TYPE_MESSAGE;

                            int broadcast_msg_count = snprintf(out_msg.message, sizeof(out_msg.message), "%s: %s", sender_name, client_msg.message);

                            out_msg.message[sizeof(out_msg.message) - 1] = '\0';
                            out_msg.message_count = (size_t)broadcast_msg_count;

                            ssize_t sent = send(ctx->clients[j].fd, &out_msg, sizeof(out_msg), 0);
                            if(sent == -1 && errno != EAGAIN)
                                fprintf(stderr, "Failed to broadcast message to client '%s': %s\n", ctx->clients[j].name, strerror(errno));
                        }
                    }
                } else if(client_msg.type == CLIENT_MESSAGE_TYPE_CONNECT) {
                    for(size_t j = 0; j < ctx->client_count; ++j) {
                        if(ctx->clients[j].fd == fd) {
                            strncpy(ctx->clients[j].name, client_msg.message, sizeof(ctx->clients[j].name));
                            ctx->clients[j].name[sizeof(ctx->clients[j].name) - 1] = '\0';
                            printf("Client fd=%d '%s' connected\n", fd, ctx->clients[j].name);
                        }
                    }
                }
            }
        }
    }

    return true;
}

int main(void) {
    Server_Context ctx = {0};
    if(!server_context_create(&ctx)) return 1;
    if(!server_listen(&ctx, "0.0.0.0", SERVER_PORT)) return 1;

    while(true) {
        if(!server_handle_events(&ctx)) return 1;
    }
}
