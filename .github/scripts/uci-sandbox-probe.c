/* CI-only engine fixture: verify the isolation actually applied to the child. */
#define _GNU_SOURCE
#include <errno.h>
#include <ifaddrs.h>
#include <sched.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <sys/resource.h>
#include <sys/statvfs.h>
#include <unistd.h>

static int check(void) {
    struct rlimit limit;
    if (getrlimit(RLIMIT_AS, &limit) || limit.rlim_max != 2048ULL * 1024 * 1024) return 1;
    if (getrlimit(RLIMIT_CPU, &limit) || limit.rlim_max != 120) return 2;
    if (getrlimit(RLIMIT_FSIZE, &limit) || limit.rlim_max != 0) return 3;
    if (prctl(PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0) != 1) return 4;
    if (getenv("GFA_SANDBOX_CANARY") || getenv("HOME")) return 5;
    if (access("/home", F_OK) == 0 || access("/etc", F_OK) == 0) return 6;
    const char *mounts[] = {"/", "/usr", "/engine", "/proc", "/dev"};
    for (unsigned i = 0; i < sizeof mounts / sizeof *mounts; i++) {
        struct statvfs fs;
        if (statvfs(mounts[i], &fs) || !(fs.f_flag & ST_RDONLY)) return 7;
    }
    FILE *status = fopen("/proc/self/status", "r");
    if (!status) return 8;
    char line[256];
    int empty_caps = 0;
    while (fgets(line, sizeof line, status)) {
        unsigned long long value;
        if (sscanf(line, "CapEff: %llx", &value) == 1 && value == 0) empty_caps = 1;
    }
    fclose(status);
    if (!empty_caps) return 9;
    struct ifaddrs *interfaces;
    if (getifaddrs(&interfaces)) return 10;
    int isolated = 1;
    for (struct ifaddrs *item = interfaces; item; item = item->ifa_next)
        if (strcmp(item->ifa_name, "lo") != 0) isolated = 0;
    freeifaddrs(interfaces);
    if (!isolated) return 11;
    if (unshare(CLONE_NEWUSER) == 0) return 12;
    return 0;
}
int main(void) {
    int failure = check();
    if (failure) {
        fprintf(stderr, "sandbox assertion %d failed\n", failure);
        return failure;
    }
    char network[128] = {0};
    ssize_t size = readlink("/proc/self/ns/net", network, sizeof network - 1);
    if (size < 0) return 13;
    char input[8194];
    while (fgets(input, sizeof input, stdin)) {
        if (strcmp(input, "uci\n") == 0) {
            printf("id name SandboxProbe %s\n", network);
            puts("option name Threads type spin default 1 min 1 max 4");
            puts("option name Hash type spin default 32 min 1 max 256");
            puts("uciok");
        } else if (strcmp(input, "isready\n") == 0) {
            puts("readyok");
        } else if (strncmp(input, "go ", 3) == 0) {
            puts("info depth 1 nodes 1 score cp 0 pv e2e4");
            puts("bestmove e2e4");
        }
        fflush(stdout);
    }
    return 0;
}
