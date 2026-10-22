FROM alpine:3.20
# htop and vim are for the manual terminal smoke test (docs/release-checklist.md).
RUN apk add --no-cache openssh openssh-server-pam htop vim
RUN adduser -D -s /bin/sh tester \
    && echo 'tester:tester' | chpasswd \
    && mkdir -p /home/tester/.ssh \
    && chmod 700 /home/tester/.ssh
COPY test_ed25519.pub /home/tester/.ssh/authorized_keys
RUN chown -R tester:tester /home/tester/.ssh && chmod 600 /home/tester/.ssh/authorized_keys
COPY sshd_config /etc/ssh/sshd_config
# Self-contained PAM stack, so the login does not depend on the distro's includes.
COPY pam_sshd /etc/pam.d/sshd
# Host keys are generated per container start, so bastion and target differ.
CMD ["sh", "-c", "ssh-keygen -A && exec /usr/sbin/sshd.pam -D -e"]
