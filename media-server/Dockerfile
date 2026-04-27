FROM alfg/nginx-rtmp

COPY nginx.conf /etc/nginx/nginx.conf

EXPOSE 1935 8088

# Bypass the base image's templated entrypoint (which silently overwrites
# /etc/nginx/nginx.conf with a default at startup) and run nginx directly
# with our config in the foreground.
ENTRYPOINT []
CMD ["nginx", "-c", "/etc/nginx/nginx.conf"]
