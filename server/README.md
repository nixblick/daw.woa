# server

Endpunkte für Feedback und Telemetrie (M0). Konzept: `docs/konzept.md`.

## Feedback-Endpunkt: zweite Instanz der bretteins-Rolle

daw.woa baut keinen eigenen Ingest. Der Feedback-Button schickt an
`https://feedback.wironair.de/api/feedback`; dort läuft dieselbe Rolle `feedback` aus
`bretteins.infra/ansible/roles/feedback` (Python-Ingest im Docker-Container, hinter dem
Edge-nginx) ein zweites Mal, mit `GITHUB_REPO=nixblick/daw.woa`.

| Datei | Zweck |
|---|---|
| `feedback/daw-woa.vars.yml` | Variablen der zweiten Instanz (Container, Port 8081, Pfade, Repo) |
| `feedback/nginx-feedback.wironair.de.conf` | vhost für den Edge-nginx, TLS per certbot-Webroot |
| `feedback/.env.example` | Platzhalter; echte Werte nur auf dem Server |
| `../docs/andre-handgriffe.md` | Runbook: DNS, Token, Ausrollen, Gegenprobe |

### Geheimnisse

Nur einer: der fine-grained Token (`nixblick/daw.woa`, Issues: Read and write). Er entsteht
und bleibt auf dem Server: `/etc/daw.woa/geheimnisse/feedback/GITHUB_TOKEN` (0640, Gruppe 999).
Nicht im Ansible-Vault, nicht in `~/.secrets/`, nicht in diesem Repo. Vault-Ordner des Repos
für Lokales: `~/.secrets/daw.woa/` (bisher leer).

### Ist die Rolle mehrinstanzfähig?

**Fast.** Parametrisiert sind bereits: Image, Container-Name, Netz, `feedback_base_dir`,
`feedback_geheim_dir`, Port, Media-Volume, Repo, Public-Base. Zweimal einbinden geht nicht
ohne Änderung, weil:

1. **Cron-Namen sind fest** (`bretteins stats retention`, `bretteins analytics push`,
   `bretteins log watch`, `bretteins feedback media retention`). `ansible.builtin.cron`
   erkennt Einträge am Namen: die zweite Instanz überschreibt die Jobs der ersten und
   zeigt sie auf `/opt/daw-woa/...`. Das ist der eigentliche Fehler.
2. **Log-Datei** `/var/log/bretteins-watchdog.log` und `log_wache_state_file` fest.
3. **Token-Pflicht im Ansible-Wert**: Die Rolle startet den Container nur, wenn
   `feedback_github_token` nicht leer ist. Unser Grundsatz (Server-Geheimnisse entstehen
   auf dem Server) braucht einen Weg „Token liegt schon als Datei“.
4. **Lagezentrum-Mount** ist immer gesetzt; für daw.woa sinnlos.
5. Der Edge-nginx kennt nur die bretteins-Namen; der vhost oben kommt dazu
   (eigenes Zertifikat, siehe Runbook).

Nicht betroffen: der Dash-Pfad `/api/feedback/dash` und Medien-URLs sind relativ zu
`FEEDBACK_PUBLIC_BASE`, die Subdomain reicht.

### Patch-Vorschlag für bretteins.infra (nicht hier committet)

Bei `feedback_instanz: bretteins` (Default) bleibt jeder Name wie heute, die laufende
bretteins-Instanz ändert sich nicht.

```diff
--- a/ansible/roles/feedback/defaults/main.yml
+++ b/ansible/roles/feedback/defaults/main.yml
@@
+# Mehrere Instanzen auf einem Host (daw.woa, 28.10.2026): Praefix fuer Cron-Namen,
+# Log-Datei und State. Default haelt alle bestehenden Namen unveraendert.
+feedback_instanz: bretteins
+feedback_log_file: "/var/log/{{ feedback_instanz }}-watchdog.log"
+# true = GITHUB_TOKEN liegt bereits als Datei in feedback_geheim_dir (auf dem Server
+# erzeugt) und kommt nicht aus dem Vault.
+feedback_github_token_auf_server: false
@@
-feedback_lagezentrum_site_dir: /srv/lagezentrum/site
+feedback_lagezentrum_site_dir: /srv/lagezentrum/site   # leer = kein Mount
@@
-log_wache_state_file: "/var/lib/bretteins/log-wache.state"
+log_wache_state_file: "/var/lib/{{ feedback_instanz }}/log-wache.state"
--- a/ansible/roles/feedback/tasks/main.yml
+++ b/ansible/roles/feedback/tasks/main.yml
@@
+- name: "Feedback | Token-Datei auf dem Server vorhanden?"
+  ansible.builtin.stat:
+    path: "{{ feedback_geheim_dir }}/GITHUB_TOKEN"
+  register: _feedback_token_datei
+  when: feedback_github_token_auf_server | bool
+  tags: [feedback]
+
 - name: "Feedback | derive activation flags"
   ansible.builtin.set_fact:
-    _feedback_gh: "{{ (feedback_github_token | default('')) | length > 0 }}"
+    _feedback_gh: >-
+      {{ ((feedback_github_token | default('')) | length > 0)
+         or (feedback_github_token_auf_server | bool
+             and (_feedback_token_datei.stat.exists | default(false))) }}
@@  (alle vier Cron-Tasks gleich)
-    name: "bretteins stats retention"
+    name: "{{ feedback_instanz }} stats retention"
-    job: "{{ feedback_base_dir }}/prune-stats.sh >> /var/log/bretteins-watchdog.log 2>&1"
+    job: "{{ feedback_base_dir }}/prune-stats.sh >> {{ feedback_log_file }} 2>&1"
@@  run ingest container, volumes:
-      - "{{ feedback_lagezentrum_site_dir }}:/data/lagezentrum:ro"
+      - "{{ feedback_lagezentrum_site_dir ~ ':/data/lagezentrum:ro' if feedback_lagezentrum_site_dir else omit }}"
```

Hinweise zum Patch:

- `omit` in einer Listen-Position greift in `docker_container` nicht sauber; sicherer ist,
  die Volume-Liste per `select` zu filtern (`[...] | select | list`). Beim Umsetzen im
  Molecule-Test von bretteins.infra prüfen.
- Der Handler `Restart feedback` läuft über `feedback_container_name` und trifft mit
  `include_role`-vars die richtige Instanz; bei `when:` dort auch
  `feedback_github_token_auf_server` berücksichtigen.
- Die Geheimnis-Task schreibt nur nicht-leere Werte; eine Datei, die André von Hand
  angelegt hat, bleibt unberührt.

Wer: Umsetzung in bretteins.infra per Ticket an Dave (`braucht:dave`), nicht aus diesem Repo.
