#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <unistd.h>

extern char **environ;

int main(int argc, char **argv)
{
    printf("hello from stdout!\n");
    fprintf(stderr, "hello from stderr!\n");

    int i = 1;
    char *s = *environ;

    for (; s; i++)
    {
        printf("%s\n", s);
        s = *(environ + i);
    }

    printf("[");
    for (int i = 0; i < argc; i++)
    {
        printf(" %s ", argv[i]);
    }
    printf("]\n");

    return EXIT_SUCCESS;
}