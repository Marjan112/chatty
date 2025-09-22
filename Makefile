all: chatty_server chatty_client

chatty_client: chatty_client.c utils.h
	gcc -o chatty_client chatty_client.c -Wall -Wextra

chatty_server: chatty_server.c utils.h
	gcc -o chatty_server chatty_server.c -Wall -Wextra
