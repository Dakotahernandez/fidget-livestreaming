FROM node:22-alpine AS builder

WORKDIR /app
COPY package*.json ./
RUN npm ci

COPY src/ src/
COPY public/ public/
COPY index.html .
COPY vite.config.js .

# Hostname browsers dial for WebTransport/QUIC ingest (UDP/4443).
# Must NOT be the CloudFront hostname \u2014 CloudFront cannot proxy UDP.
# Point this at a DNS record that resolves directly to the EC2 instance
# with UDP/4443 open in its security group (e.g. ingest.fidgetlive.com).
ARG VITE_WEBTRANSPORT_HOST=""
ENV VITE_WEBTRANSPORT_HOST=$VITE_WEBTRANSPORT_HOST

RUN npm run build

FROM nginx:alpine

COPY --from=builder /app/dist /usr/share/nginx/html
COPY nginx.conf /etc/nginx/conf.d/default.conf

EXPOSE 3000
CMD ["nginx", "-g", "daemon off;"]
