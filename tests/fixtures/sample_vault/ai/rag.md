---
title: Retrieval Augmented Generation
aliases:
  - RAG
  - Hybrid Retrieval
tags:
  - ai
  - retrieval/hybrid
status: active
---

# Retrieval Augmented Generation

Hybrid retrieval combines lexical BM25 evidence with semantic vector evidence. A calibrated retrieval pipeline keeps each retriever's native score separate and combines rank positions with reciprocal rank fusion.

For this vault, RAG is only a retrieval layer. It is not a hosted AI tool and it does not generate global graph summaries.

Related notes: [[obsidian]], [[lost-in-the-middle]], and [[../aws/service-connect|Service Connect]].

## Reciprocal Rank Fusion

RRF rewards chunks that appear near the top of both BM25 and vector result lists.
