/* Bounded ARMv7/musl sandbox negative-control fixture.
 * This deliberately blocks getppid (an otherwise permitted syscall) to
 * prove *effective* seccomp, while permitting process, Unix IPC and
 * AF_INET socket creation. It is NOT a production NetworkProcess policy.
 * Only run in a fresh, unprivileged Bubblewrap namespace.
 */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <linux/audit.h>
#include <linux/filter.h>
#include <linux/seccomp.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/un.h>
#include <unistd.h>

static void die(const char *message)
{
    fprintf(stderr, "POLICY_PROBE_FAIL %s errno=%d (%s)\n",
            message, errno, strerror(errno));
    exit(1);
}

static int status_number(const char *key)
{
    FILE *file = fopen("/proc/self/status", "r");
    char line[160];
    int result = -1;

    if (!file)
        die("proc-status");
    while (fgets(line, sizeof(line), file)) {
        size_t size = strlen(key);
        if (!strncmp(line, key, size) && line[size] == ':') {
            result = atoi(line + size + 1);
            break;
        }
    }
    fclose(file);
    return result;
}

static void verify_files(const char *allowed, const char *denied)
{
    char first = 0;
    int file = open(allowed, O_RDONLY | O_CLOEXEC);
    if (file < 0)
        die("allowed-file-open");
    if (read(file, &first, 1) != 1 || first != 'Q')
        die("allowed-file-content");
    close(file);
    errno = 0;
    if (access(denied, F_OK) == 0) {
        fputs("POLICY_PROBE_FAIL forbidden-host-file-visible\n", stderr);
        exit(1);
    }
    if (errno != ENOENT)
        die("forbidden-host-file-not-ENOENT");
    puts("MOUNT_POLICY_PASS allowed-file=read forbidden-host-file=ENOENT");
}

static void verify_sockets(const char *denied)
{
    int server = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    int client = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    int inet = socket(AF_INET, SOCK_STREAM | SOCK_CLOEXEC, 0);
    struct sockaddr_un address = { .sun_family = AF_UNIX };
    int accepted;

    if (server < 0 || client < 0 || inet < 0)
        die("allowed-socket-create");
    close(inet);
    if (snprintf(address.sun_path, sizeof(address.sun_path),
                 "/tmp/nura-policy-%ld.sock", (long)getpid())
        >= (int)sizeof(address.sun_path))
        die("socket-path-too-long");
    if (bind(server, (void *)&address, sizeof(address)) != 0)
        die("allowed-unix-bind");
    if (listen(server, 1) != 0)
        die("allowed-unix-listen");
    if (connect(client, (void *)&address, sizeof(address)) != 0)
        die("allowed-unix-connect");
    accepted = accept(server, NULL, NULL);
    if (accepted < 0)
        die("allowed-unix-accept");
    close(accepted);
    close(client);
    close(server);
    unlink(address.sun_path);

    int unwanted = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (unwanted < 0)
        die("denied-socket-create");
    memset(&address, 0, sizeof(address));
    address.sun_family = AF_UNIX;
    if (snprintf(address.sun_path, sizeof(address.sun_path), "%s", denied)
        >= (int)sizeof(address.sun_path))
        die("denied-socket-path-too-long");
    errno = 0;
    int result = connect(unwanted, (void *)&address, sizeof(address));
    int saved_errno = errno;
    close(unwanted);
    if (result == 0) {
        fputs("POLICY_PROBE_FAIL forbidden-host-socket-connected\n", stderr);
        exit(1);
    }
    if (saved_errno != ENOENT) {
        errno = saved_errno;
        die("forbidden-host-socket-not-ENOENT");
    }
    puts("SOCKET_POLICY_PASS local-unix=connect inet=create forbidden-host=ENOENT");
}

static void verify_seccomp(void)
{
    const int before = status_number("Seccomp_filters");
    const long parent = syscall(SYS_getppid);
    struct sock_filter filter[] = {
        BPF_STMT(BPF_LD | BPF_W | BPF_ABS,
                 offsetof(struct seccomp_data, arch)),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, AUDIT_ARCH_ARM, 1, 0),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_KILL_PROCESS),
        BPF_STMT(BPF_LD | BPF_W | BPF_ABS,
                 offsetof(struct seccomp_data, nr)),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, SYS_getppid, 0, 1),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ERRNO | EACCES),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, SYS_ptrace, 0, 1),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ERRNO | EPERM),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW),
    };
    struct sock_fprog program = {
        .len = (unsigned short)(sizeof(filter) / sizeof(filter[0])),
        .filter = filter,
    };

    /* PID 1 in a private PID namespace correctly has getppid() == 0. */
    if (before < 0 || parent < 0)
        die("baseline-seccomp-status-or-getppid");
    if (prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0)
        die("no-new-privs");
    if (syscall(SYS_seccomp, SECCOMP_SET_MODE_FILTER, 0, &program) != 0)
        die("seccomp-install");
    if (status_number("NoNewPrivs") != 1 ||
        status_number("Seccomp") != SECCOMP_MODE_FILTER ||
        status_number("Seccomp_filters") != before + 1)
        die("seccomp-not-observed-after-install");

    errno = 0;
    const long blocked = syscall(SYS_getppid);
    const int blocked_errno = errno;
    const long allowed = syscall(SYS_getpid);
    if (blocked != -1 || blocked_errno != EACCES || allowed <= 0)
        die("seccomp-denial-or-allowance-mismatch");
    printf("SECCOMP_POLICY_PASS nnp=1 mode=2 filters=%d->%d "
           "getppid=EACCES getpid=allowed\n",
           before, before + 1);
}

int main(int argc, char **argv)
{
    if (argc != 4) {
        fputs("usage: probe ALLOWED_FILE HIDDEN_HOST_FILE HIDDEN_HOST_SOCKET\n",
              stderr);
        return 64;
    }
    verify_files(argv[1], argv[2]);
    verify_sockets(argv[3]);
    verify_seccomp();
    puts("NETWORK_POLICY_FIXTURE=PASS not-a-WebKit-launch");
    return 0;
}
