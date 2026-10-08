{{- define "vda.name" -}}
{{- printf "%s-%s" .Release.Name .Chart.Name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- define "vda.serviceAccountName" -}}
{{- if .Values.serviceAccount.create -}}
{{- default (include "vda.name" .) .Values.serviceAccount.name -}}
{{- else -}}
{{- default "default" .Values.serviceAccount.name -}}
{{- end -}}
{{- end -}}
{{- define "vda.labels" -}}
app.kubernetes.io/name: visp-db-access
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end -}}
