# Handgriffe für André: Feedback-Endpunkt daw.woa

Ziel: `https://feedback.wironair.de/api/feedback` legt Issues in `nixblick/daw.woa` an.
Hintergrund und Patch: `server/README.md`. Reihenfolge = Ausführung. Kein Geheimniswert
steht in dieser Datei; den Token tippst/fügst du nur auf dem Server ein.

Vorbelegt (bei Bedarf oben einmal anpassen):

```bash
VPS=lfs001               # Hetzner-VPS mit dem bretteins-Edge
SUB=feedback.wironair.de
```

## 100 DNS der Subdomain

### 101 · Host: Notebook
IPv4/IPv6 des VPS ermitteln:

```bash
dig +short A bretteins.de; dig +short AAAA bretteins.de
```

Erwartet: je eine Adresse (die des VPS).

### 110 · Host: Browser (DNS-Verwaltung von wironair.de)
Zwei Einträge anlegen, TTL 3600:

```
feedback   A      <IPv4 aus 101>
feedback   AAAA   <IPv6 aus 101>
```

### 120 · Host: Notebook
Gegenprobe (kann bis zur TTL dauern):

```bash
dig +short A feedback.wironair.de; dig +short AAAA feedback.wironair.de
```

Erwartet: dieselben Adressen wie in 101.

## 200 Fine-grained Token (nur auf dem Server)

### 201 · Host: Browser
<https://github.com/settings/personal-access-tokens/new>

- Token name: `daw-woa-feedback`
- Expiration: 90 days
- Resource owner: `nixblick`
- Repository access: Only select repositories → `nixblick/daw.woa`
- Permissions → Repository → Issues: **Read and write** (sonst nichts; Metadata: Read kommt automatisch)

„Generate token“, Wert in die Zwischenablage. Nirgends sonst ablegen.

### 210 · Host: VPS (`ssh lfs001`)

```bash
sudo install -d -m 0750 -o root -g 999 /etc/daw.woa/geheimnisse/feedback
sudo vi /etc/daw.woa/geheimnisse/feedback/GITHUB_TOKEN
```

In vi: `i`, Token einfügen, `Esc`, `:wq`. Eine Zeile, kein Leerzeichen.

### 220 · Host: VPS

```bash
sudo chown root:999 /etc/daw.woa/geheimnisse/feedback/GITHUB_TOKEN
sudo chmod 0640 /etc/daw.woa/geheimnisse/feedback/GITHUB_TOKEN
sudo stat -c '%a %U:%g %s' /etc/daw.woa/geheimnisse/feedback/GITHUB_TOKEN
```

Erwartet: `640 root:999 <Größe>` mit Größe um 94 (github_pat_…). Inhalt nicht ausgeben.

### 230 · Host: Notebook
Ablaufdatum in `creavisions.stack/regeln/tokens.tsv` eintragen (ohne Wert) und
Kalendertermin 7 Tage vorher, wie bei `bin/token-erneuern.sh`.

## 300 Zweite Instanz ausrollen

Voraussetzung: der Patch aus `server/README.md` ist in bretteins.infra gemergt
(Ticket `braucht:dave`), und `server/feedback/daw-woa.vars.yml` liegt dort als
Instanz-Variablen samt `include_role`-Block in `site.yml`.

### 301 · Host: Notebook
Trockenlauf:

```bash
cd ~/GitHub/nixblick/bretteins.infra/ansible
ansible-playbook -i inventories/production site.yml --tags feedback-daw --check --diff
```

Erwartet: neue Tasks für `daw-woa-feedback`, **keine** Änderung an Cron-Einträgen
`bretteins …` und am Container `bretteins-feedback`.

### 310 · Host: Notebook

```bash
ansible-playbook -i inventories/production site.yml --tags feedback-daw
```

Erwartet: `failed=0`.

### 320 · Host: VPS

```bash
docker ps --filter name=daw-woa-feedback --format '{{.Names}} {{.Status}}'
sudo crontab -l | grep -E 'bretteins|daw-woa'
```

Erwartet: `daw-woa-feedback Up … (healthy)`; bretteins-Cronzeilen unverändert.

### 330 · Host: VPS
vhost in den Edge legen (Vorlage `server/feedback/nginx-feedback.wironair.de.conf`,
Pfad wie die übrigen Edge-Confs), zuerst nur der Port-80-Block, dann Zertifikat:

```bash
sudo certbot certonly --webroot -w /srv/acme -d feedback.wironair.de
```

Erwartet: `Successfully received certificate`. Webroot-Pfad = `edge_acme_webroot` auf
dem Host; Zertifikat dorthin legen, wo der Edge-Container `/etc/nginx/certs` einhängt.
Dann den 443-Block aktivieren:

```bash
docker exec bretteins-edge nginx -t && docker exec bretteins-edge nginx -s reload
```

Erwartet: `syntax is ok`, `test is successful`.

## 400 Gegenprobe per curl

### 401 · Host: Notebook

```bash
curl -sS -o /dev/null -w '%{http_code}\n' https://feedback.wironair.de/healthz
```

Erwartet: `200`.

### 410 · Host: Notebook
Test-Issue anlegen:

```bash
curl -sS -X POST https://feedback.wironair.de/api/feedback -H 'Content-Type: application/json' -d '{"kind":"feedback","title":"Gegenprobe Feedback-Endpunkt","body":"Gegenprobe Feedback-Endpunkt (bitte schliessen)","app":"woa 0.1.0"}' -w '\n%{http_code}\n'
```

Erwartet: `201` (ohne DB, synchron) mit Issue-Nummer im JSON.

### 420 · Host: Notebook

```bash
gh issue list -R nixblick/daw.woa --search "Gegenprobe Feedback-Endpunkt" --state open
```

Erwartet: genau ein Issue (Label `feedback-ungeprueft`, weil die Installation nicht in
der Vertrauensliste steht). Danach schließen:

```bash
gh issue close <nr> -R nixblick/daw.woa -c "Gegenprobe ok"
```
