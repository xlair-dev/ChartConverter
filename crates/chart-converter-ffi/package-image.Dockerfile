FROM alpine:3.22

LABEL org.opencontainers.image.source="https://github.com/xlair-dev/ChartConverter"
LABEL org.opencontainers.image.title="ChartConverter C API packages"

COPY packages/ /packages/
COPY archives/ /archives/
COPY README.md /README.md

CMD ["sh"]
