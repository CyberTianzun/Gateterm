/* Verification tool only: never copy this executable into the service image. */
#define _GNU_SOURCE
#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/syscall.h>
#include <unistd.h>

static void require(int pass, const char *what) {
    if (!pass) { perror(what); exit(1); }
}

static void require_cgroup(const char *path, const char *first, const char *second) {
    FILE *control = fopen(path, "r");
    require(control != NULL, path);
    char actual_first[32] = "", actual_second[32] = "", extra[2];
    int count = fscanf(control, "%31s %31s %1s", actual_first, actual_second, extra);
    require(!ferror(control), "read cgroup control");
    fclose(control);
    if (count != (second ? 2 : 1) || strcmp(actual_first, first)
            || (second && strcmp(actual_second, second))) {
        fprintf(stderr, "FAIL: %s expected %s%s%s; got %s %s (tokens=%d)\n",
                path, first, second ? " " : "", second ? second : "",
                actual_first, actual_second, count);
        exit(1);
    }
}

int main(void) {
    require(getuid() == 65532 && getgid() == 65532, "non-root identity");
    const int flags[] = {0, SOCK_NONBLOCK, SOCK_CLOEXEC, SOCK_NONBLOCK | SOCK_CLOEXEC};
    const int families[] = {AF_INET, AF_INET6};
    for (unsigned f = 0; f < 2; f++) {
        const int forbidden_protocols[] = {132, 262}; /* SCTP, MPTCP */
        for (unsigned i = 0; i < 2; i++) {
            errno = 0;
            require(socket(families[f], SOCK_STREAM, forbidden_protocols[i]) == -1 && errno == EPERM,
                    "non-TCP stream protocols denied before kernel protocol lookup");
        }
        for (unsigned i = 0; i < 4; i++) {
            errno = 0;
            require(socket(families[f], SOCK_DGRAM | flags[i], 0) == -1 && errno == EPERM,
                    "IPv4/IPv6 UDP creation must be EPERM including flags");
        }
        int fd = socket(families[f], SOCK_STREAM | SOCK_CLOEXEC, 0);
        require(fd >= 0, "TCP socket creation");
        struct sockaddr_in addr = {.sin_family = AF_INET, .sin_port = htons(9)};
        inet_pton(AF_INET, "127.0.0.1", &addr.sin_addr);
        errno = 0;
        require(connect(fd, (struct sockaddr *)&addr, sizeof(addr)) == -1 && errno == EPERM,
                "connect must be EPERM");
        errno = 0;
        require(sendto(fd, "probe", 5, MSG_FASTOPEN, (struct sockaddr *)&addr, sizeof(addr)) == -1 && errno == EPERM,
                "TCP Fast Open sendto connect bypass denied");
        struct iovec iov = {.iov_base = "probe", .iov_len = 5};
        struct msghdr message = {.msg_name = &addr, .msg_namelen = sizeof(addr), .msg_iov = &iov, .msg_iovlen = 1};
        errno = 0;
        require(sendmsg(fd, &message, MSG_FASTOPEN) == -1 && errno == EPERM,
                "TCP Fast Open sendmsg connect bypass denied");
        struct mmsghdr messages = {.msg_hdr = message};
        errno = 0;
        require(sendmmsg(fd, &messages, 1, MSG_FASTOPEN) == -1 && errno == EPERM,
                "TCP Fast Open sendmmsg connect bypass denied");
        close(fd);
    }
    errno = 0;
    require(syscall(SYS_io_uring_setup, 0, NULL) == -1 && errno == EPERM, "io_uring bypass denied");
    errno = 0;
    require(syscall(SYS_fork) == -1 && errno == EPERM, "process creation denied");
    errno = 0;
    require(socket(AF_UNIX, SOCK_STREAM, 0) == -1 && errno == EPERM, "Unix socket creation denied");
    errno = 0;
    require(open("/write-probe", O_CREAT | O_WRONLY, 0600) == -1 && errno == EROFS,
            "root filesystem is read-only");
    FILE *status = fopen("/proc/self/status", "r");
    require(status != NULL, "read process security status");
    char line[512]; int nnp = 0, caps = 0, bounding = 0, filter = 0;
    while (fgets(line, sizeof(line), status)) {
        if (!strcmp(line, "NoNewPrivs:\t1\n")) nnp = 1;
        if (!strcmp(line, "CapEff:\t0000000000000000\n")) caps = 1;
        if (!strcmp(line, "CapBnd:\t0000000000000000\n")) bounding = 1;
        if (!strcmp(line, "Seccomp:\t2\n")) filter = 1;
    }
    fclose(status);
    require(nnp && caps && bounding && filter, "no-new-privileges, effective/bounding capabilities and seccomp");
    require_cgroup("/sys/fs/cgroup/memory.max", "134217728", NULL);
    require_cgroup("/sys/fs/cgroup/memory.swap.max", "0", NULL);
    require_cgroup("/sys/fs/cgroup/pids.max", "64", NULL);
    require_cgroup("/sys/fs/cgroup/cpu.max", "50000", "100000");
    puts("PASS: uid/gid65532, read-only root, zero capabilities, no-new-privileges, seccomp filter; TCP sockets allowed; connect, TCP Fast Open, IPv4/IPv6 UDP (all flags), Unix sockets, io_uring and fork denied");
    puts("PASS: in-container cgroup v2 memory=128MiB, swap=0, pids=64, cpu=50000/100000 (0.5)");
    return 0;
}
