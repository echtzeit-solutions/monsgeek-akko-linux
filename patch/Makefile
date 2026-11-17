# Host-side tests for the shared patch headers. The firmware itself is built
# from firmwares/*/patch/Makefile.
CC ?= cc

.PHONY: test clean

test: test_memaccess
	./test_memaccess

test_memaccess: test_memaccess.c memaccess.h
	$(CC) -std=c11 -Wall -Wextra -Werror -o $@ test_memaccess.c

clean:
	rm -f test_memaccess
