---
title: Saturation du contexte
alias: "Memoire de contexte"
aliases: ["Contexte long", "Fenetre de contexte"]
tags: ai/context retrieval
---

# Saturation du contexte

La saturation du contexte arrive quand un agent charge trop de notes a la fois. La bonne reaction n'est pas de lire tout le vault, mais de recuperer quelques passages courts avec une recherche hybride.

Cette note relie la question francaise aux idees de [[lost-in-the-middle|lost in the middle]] et a la note [[rag|RAG]]. Elle sert aussi de requete vectorielle utile pour "Comment eviter la saturation du contexte ?".

## Strategie pratique

Pour eviter la saturation du contexte, il faut chercher avant de lire. Une recherche lexicale trouve les noms exacts, tandis qu'une recherche vectorielle retrouve les formulations vagues ou les questions conceptuelles. La fusion garde les meilleurs indices des deux methodes.

Un agent devrait lire seulement les chunks les mieux classes, verifier les chemins et les titres, puis citer les fichiers consultes. Cette discipline limite le bruit et garde la fenetre de contexte disponible pour le raisonnement utile.

## Long exemple

Ce long passage force le chunker a decouper une section trop grande par paragraphes sans couper les blocs Markdown importants. Chaque paragraphe repete volontairement le meme theme avec des formulations legerement differentes afin de creer un document assez long pour les tests.

La premiere regle consiste a traiter le vault comme une base documentaire locale. Le moteur indexe les fichiers Markdown, extrait les titres, les alias, les tags et les wikilinks, puis prepare des chunks lisibles par un agent. Cette approche evite de transformer le vault en systeme magique.

La deuxieme regle consiste a privilegier les chemins directs. Si la question contient un nom de service, une erreur, une classe, une commande ou un acronyme, la recherche BM25 doit trouver le passage precis. Elle est utile quand les mots importants sont deja connus.

La troisieme regle consiste a utiliser les embeddings locaux quand la question est vague. Une personne peut demander comment reduire la charge cognitive, comment eviter la saturation du contexte, ou comment retrouver une idee voisine. La recherche vectorielle donne alors un signal complementaire.

La quatrieme regle consiste a fusionner les rangs plutot que les scores bruts. Les scores BM25 et les similarites vectorielles n'ont pas la meme echelle. Reciprocal Rank Fusion additionne des contributions simples fondees sur la position dans chaque liste.

La cinquieme regle consiste a garder le graphe leger. Les wikilinks et backlinks peuvent ajouter une note voisine utile, mais ils ne doivent jamais dominer la pertinence lexicale ou vectorielle. Le graphe sert d'extension locale, pas de generation de connaissances.

La sixieme regle consiste a proteger la confidentialite. Le contenu reste sur la machine locale, les embeddings sont generes par FastEmbed, et les resultats sont stockes dans SQLite et Tantivy. Aucune API LLM hebergee n'est necessaire pour indexer ou chercher.

La septieme regle consiste a conserver des chunks comprehensibles seuls. Le chemin du fichier, le titre de la note, le chemin de titre Markdown et les lignes d'origine aident un agent a comprendre le contexte sans ouvrir tout le document.

La huitieme regle consiste a rester deterministe. Deux executions sur le meme vault doivent produire les memes chunks, les memes identifiants et les memes relations, sauf si les fichiers Markdown changent. Cette propriete rend les tests plus simples.

La neuvieme regle consiste a separer les responsabilites. SQLite stocke les metadonnees, les chunks, les liens, les alias, les tags et les embeddings. Tantivy fournit la recherche BM25 rapide. Le code Rust garde ces couches explicites.

La dixieme regle consiste a limiter les effets de bord. L'outil ne modifie pas les notes Obsidian. Il ecrit seulement ses fichiers locaux d'index dans le repertoire `.obsidian-kb`, plus le fichier de configuration demande par l'utilisateur.

La onzieme regle consiste a rendre les resultats explicables. Un resultat de recherche doit montrer son rang final, son score final, son rang BM25, son score BM25, son rang vectoriel, son score vectoriel, et le boost eventuel du graphe.

La douzieme regle consiste a tester les cas limites avec un petit vault synthetique. Ce vault contient des notes en francais et en anglais, des wikilinks, des alias, des tags, un lien non resolu, et cette longue section qui oblige le decoupage.

La treizieme regle consiste a accepter un MVP simple. Une recherche vectorielle brute force suffit pour un petit vault, car elle reste locale, lisible et facile a verifier. Le remplacement par un index approximatif peut attendre un volume beaucoup plus grand.

La quatorzieme regle consiste a garder les frontieres nettes. Le programme n'ecrit pas de resume dans Obsidian, ne cree pas de nouvelles notes, et ne modifie pas les fichiers Markdown existants. Il construit seulement une couche de recuperation locale.

La quinzieme regle consiste a utiliser le graphe uniquement comme voisinage. Une note liee directement peut ajouter un contexte utile, par exemple une note sur RAG proche d'une note sur Obsidian, mais cette relation ne remplace jamais le score de recherche.

La seizieme regle consiste a conserver les blocs de code intacts. Quand une section contient une cloture Markdown, le chunker doit eviter de couper le bloc au milieu, meme si cela produit un chunk un peu plus long que la cible ideale.
