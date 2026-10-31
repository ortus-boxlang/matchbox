#define _GNU_SOURCE
#include <signal.h>
#include <string.h>
#include <sys/syscall.h>
#include <sys/uio.h>
#include <unistd.h>

/* The controller attaches/enables/disables hardware counters while the whole
 * target is stopped. Markers must be separate stdout writes containing the
 * complete token; the hook is for synchronous, single-region harnesses. */
static void marker(int fd, const void *buffer, size_t length) {
    static const char start[] = "__ICOUNT_START__";
    static const char end[] = "__ICOUNT_END__";
    if (fd == STDOUT_FILENO &&
        (memmem(buffer, length, start, sizeof(start) - 1) ||
         memmem(buffer, length, end, sizeof(end) - 1))) {
        syscall(SYS_kill, syscall(SYS_getpid), SIGSTOP);
    }
}

ssize_t write(int fd, const void *buffer, size_t length) {
    marker(fd, buffer, length);
    return syscall(SYS_write, fd, buffer, length);
}

ssize_t writev(int fd, const struct iovec *iov, int count) {
    for (int i = 0; i < count; i++) {
        marker(fd, iov[i].iov_base, iov[i].iov_len);
    }
    return syscall(SYS_writev, fd, iov, count);
}
