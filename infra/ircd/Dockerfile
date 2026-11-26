# SPDX-License-Identifier: GPL-3.0-or-later
# Eigenständiger ngIRCd-Container (GPLv2+, strikt prozess- und netzwerkgetrennt)

FROM alpine:3.20
RUN apk add --no-cache ngircd netcat-openbsd \
    && mkdir -p /run/ngircd && chown -R irc:irc /run/ngircd

COPY ngircd.conf /etc/ngircd/ngircd.conf
RUN chown irc:irc /etc/ngircd/ngircd.conf && chmod 0640 /etc/ngircd/ngircd.conf

EXPOSE 6667/tcp
CMD ["ngircd", "-n", "-f", "/etc/ngircd/ngircd.conf"]
