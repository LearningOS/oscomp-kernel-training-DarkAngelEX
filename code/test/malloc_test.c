#include <stdio.h>
#include <stdlib.h>

int main() {
    printf("call malloc\n");
    size_t* ptr = malloc(8);
    if (!ptr) {
        printf("malloc fail!\n");
        exit(-1);
    }
    printf("ptr: %p", ptr);
    *ptr = 1;
    printf("call free\n");
    free(ptr);
    printf("end\n");
    return 0;
}