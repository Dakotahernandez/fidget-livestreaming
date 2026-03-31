FROM alfg/nginx-rtmp

COPY nginx.conf /etc/nginx/nginx.conf
COPY scripts/ /scripts/
RUN chmod +x /scripts/*.sh

EXPOSE 1935 8088
