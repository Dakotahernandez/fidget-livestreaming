FROM ossrs/srs:5

# Replace the upstream config with our low-latency tuned config.
COPY srs.conf /usr/local/srs/conf/srs.conf

EXPOSE 1935 8088 1985

# SRS expects to be invoked from its install directory.
WORKDIR /usr/local/srs
ENTRYPOINT []
CMD ["./objs/srs", "-c", "conf/srs.conf"]
