#!/usr/bin/env bash
set -euo pipefail

DOMAIN="agentdeaddrop.com"

echo "==> Installing nginx and certbot..."
apt-get update
apt-get install -y nginx certbot python3-certbot-nginx

echo "==> Writing nginx config for ${DOMAIN}..."
cat > /etc/nginx/sites-available/"${DOMAIN}" <<'NGINX'
server {
    listen 80;
    server_name agentdeaddrop.com;

    location / {
        proxy_pass http://127.0.0.1:3000;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
NGINX

echo "==> Enabling site and removing default..."
ln -sf /etc/nginx/sites-available/"${DOMAIN}" /etc/nginx/sites-enabled/"${DOMAIN}"
rm -f /etc/nginx/sites-enabled/default
nginx -t
systemctl reload nginx

echo "==> Configuring UFW firewall..."
ufw allow OpenSSH
ufw allow 'Nginx Full'
ufw --force enable

echo "==> Obtaining SSL certificate..."
certbot --nginx -d "${DOMAIN}" --non-interactive --agree-tos --redirect -m chadburnette@me.com

echo "==> Done. Verify with: curl https://${DOMAIN}/health"
