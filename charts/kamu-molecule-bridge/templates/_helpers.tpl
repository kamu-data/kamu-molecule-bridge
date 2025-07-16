{{/*
Expand the name of the chart.
*/}}
{{- define "kamu-molecule-bridge.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
We truncate at 63 chars because some Kubernetes name fields are limited to this (by the DNS naming spec).
If release name contains chart name it will be used as a full name.
*/}}
{{- define "kamu-molecule-bridge.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{/*
Create chart name and version as used by the chart label.
*/}}
{{- define "kamu-molecule-bridge.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Common labels
*/}}
{{- define "kamu-molecule-bridge.labels" -}}
helm.sh/chart: {{ include "kamu-molecule-bridge.chart" . }}
{{ include "kamu-molecule-bridge.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{/*
Selector labels
*/}}
{{- define "kamu-molecule-bridge.selectorLabels" -}}
app.kubernetes.io/name: {{ include "kamu-molecule-bridge.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
Create the name of the service account to use
*/}}
{{- define "kamu-molecule-bridge.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}
{{- default (include "kamu-molecule-bridge.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.serviceAccount.name }}
{{- end }}
{{- end }}


{{/*
Renders the content of the configmap
*/}}
{{- define "kamu-molecule-bridge.config.yaml" -}}
{{- $root := . -}}
config.yaml: | {{ .Values.app.config | toYaml | nindent 2 }}
{{- end -}}

{{/*
Computes a short hash of the configmap content to re-create it and trigger the re-deploy upon any change
*/}}
{{- define "kamu-molecule-bridge.config.sha" -}}
{{ include "kamu-molecule-bridge.config.yaml" . | toJson | sha256sum | trunc 10 }}
{{- end -}}

{{- define "kamu-molecule-bridge.awsCredentialsSecretName" -}}
{{- if .Values.app.awsCredentials.existingSecret }}
{{- .Values.app.awsCredentials.existingSecret }}
{{- else }}
{{- printf "%s-aws-credentials" (include "kamu-molecule-bridge.fullname" .) }}
{{- end }}
{{- end }}
