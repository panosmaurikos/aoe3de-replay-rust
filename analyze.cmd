@echo off
rem Drag a .age3Yrec onto this file, or run: analyze "path\to\game.age3Yrec"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0analyze.ps1" %*
