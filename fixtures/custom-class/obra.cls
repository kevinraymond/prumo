% Fixture: a small "building documentation" class in the spirit of the
% original request: wraps article, pulls in its own package set.
\NeedsTeXFormat{LaTeX2e}
\ProvidesClass{obra}[2026/11/06 v0.1 fixture class]
\LoadClass[11pt]{article}
\RequirePackage[a4paper,margin=25mm]{geometry}
\RequirePackage{booktabs}
\RequirePackage{siunitx}
\RequirePackage{fancyhdr}
\RequirePackage{enumitem}   % never used by the fixture document
\pagestyle{fancy}
\fancyhf{}
\fancyhead[L]{\textsc{obra}}
\fancyfoot[C]{\thepage}
\newcommand\obratitle[1]{\section*{#1}}
