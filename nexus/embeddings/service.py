"""
Embedding service for generating text embeddings using sentence-transformers.

This module provides a high-performance embedding service with:
- Async support for batch operations
- Model caching and pooling
- Configurable model selection
- Error handling and retry logic
"""

import asyncio
from functools import lru_cache
from typing import List, Optional, Union
import numpy as np
from sentence_transformers import SentenceTransformer
from loguru import logger


class EmbeddingService:
    """
    Service for generating text embeddings using sentence-transformers.

    Uses the all-MiniLM-L6-v2 model which provides:
    - 384-dimensional embeddings
    - Fast inference (~1000 docs/sec on CPU)
    - Good quality for semantic similarity
    - Support for 100+ languages

    Attributes:
        model_name: The HuggingFace model identifier
        dimension: Embedding vector dimension (384 for all-MiniLM-L6-v2)
        _model: Cached sentence-transformers model instance
    """

    # Default model configuration
    DEFAULT_MODEL = "all-MiniLM-L6-v2"
    DIMENSION = 384

    def __init__(
        self,
        model_name: str = DEFAULT_MODEL,
        device: Optional[str] = None,
        cache_dir: Optional[str] = None,
    ):
        """
        Initialize the embedding service.

        Args:
            model_name: HuggingFace model identifier
            device: Device to run model on ('cpu', 'cuda', etc.)
            cache_dir: Directory to cache downloaded models
        """
        self.model_name = model_name
        self.dimension = self.DIMENSION
        self.device = device
        self.cache_dir = cache_dir
        self._model: Optional[SentenceTransformer] = None
        self._lock = asyncio.Lock()

    @property
    def model(self) -> SentenceTransformer:
        """
        Lazy-load and cache the sentence-transformers model.

        Returns:
            SentenceTransformer model instance

        Raises:
            RuntimeError: If model fails to load
        """
        if self._model is None:
            try:
                logger.info(f"Loading embedding model: {self.model_name}")
                self._model = SentenceTransformer(
                    self.model_name,
                    device=self.device,
                    cache_folder=self.cache_dir,
                )
                logger.info(f"Model loaded successfully: {self.model_name}")
            except Exception as e:
                logger.error(f"Failed to load embedding model: {e}")
                raise RuntimeError(f"Failed to load embedding model {self.model_name}: {e}")
        return self._model

    async def encode(
        self,
        texts: Union[str, List[str]],
        normalize: bool = True,
        batch_size: int = 32,
        show_progress: bool = False,
    ) -> np.ndarray:
        """
        Encode text(s) into embedding vectors.

        Args:
            texts: Single text string or list of text strings
            normalize: Whether to normalize embeddings to unit length
            batch_size: Batch size for encoding multiple texts
            show_progress: Whether to show progress bar for batches

        Returns:
            numpy array of shape (n_texts, dimension) with dtype float32

        Raises:
            ValueError: If texts is empty or invalid type
            RuntimeError: If encoding fails

        Examples:
            >>> service = EmbeddingService()
            >>> embedding = await service.encode("Hello world")
            >>> embedding.shape
            (384,)

            >>> embeddings = await service.encode(["Hello", "World"])
            >>> embeddings.shape
            (2, 384)
        """
        if not texts:
            raise ValueError("Cannot encode empty texts")

        # Normalize input to list
        single_input = isinstance(texts, str)
        texts_list = [texts] if single_input else texts

        if not isinstance(texts_list, list) or not all(
            isinstance(t, str) for t in texts_list
        ):
            raise ValueError("texts must be a string or list of strings")

        try:
            # Run encoding in thread pool to avoid blocking
            loop = asyncio.get_event_loop()
            embeddings = await loop.run_in_executor(
                None,
                lambda: self.model.encode(
                    texts_list,
                    normalize_embeddings=normalize,
                    batch_size=batch_size,
                    show_progress_bar=show_progress,
                    convert_to_numpy=True,
                ),
            )

            # Ensure float32 for sqlite-vec compatibility
            embeddings = embeddings.astype(np.float32)

            # Return single vector for single input
            if single_input:
                return embeddings[0]

            return embeddings

        except Exception as e:
            logger.error(f"Failed to encode texts: {e}")
            raise RuntimeError(f"Encoding failed: {e}")

    async def encode_batch(
        self,
        texts: List[str],
        normalize: bool = True,
        batch_size: int = 32,
    ) -> List[np.ndarray]:
        """
        Encode a batch of texts, returning a list of embedding vectors.

        This is a convenience method that processes texts in batches
        and returns individual embedding vectors.

        Args:
            texts: List of text strings
            normalize: Whether to normalize embeddings
            batch_size: Batch size for processing

        Returns:
            List of numpy arrays, each of shape (dimension,)

        Examples:
            >>> service = EmbeddingService()
            >>> embeddings = await service.encode_batch(["a", "b", "c"])
            >>> len(embeddings)
            3
        """
        if not texts:
            return []

        embeddings_array = await self.encode(texts, normalize, batch_size)
        return [embeddings_array[i] for i in range(len(texts))]

    def compute_similarity(
        self,
        embedding1: np.ndarray,
        embedding2: np.ndarray,
    ) -> float:
        """
        Compute cosine similarity between two embeddings.

        Args:
            embedding1: First embedding vector
            embedding2: Second embedding vector

        Returns:
            Cosine similarity score between -1 and 1

        Examples:
            >>> service = EmbeddingService()
            >>> e1 = np.random.rand(384)
            >>> e2 = np.random.rand(384)
            >>> similarity = service.compute_similarity(e1, e2)
        """
        # Normalize vectors
        e1_norm = embedding1 / (np.linalg.norm(embedding1) + 1e-9)
        e2_norm = embedding2 / (np.linalg.norm(embedding2) + 1e-9)

        # Cosine similarity
        return float(np.dot(e1_norm, e2_norm))

    async def compute_similarity_batch(
        self,
        query_embedding: np.ndarray,
        corpus_embeddings: np.ndarray,
    ) -> np.ndarray:
        """
        Compute cosine similarities between query and corpus embeddings.

        Args:
            query_embedding: Query vector of shape (dimension,)
            corpus_embeddings: Corpus vectors of shape (n, dimension)

        Returns:
            Array of similarity scores of shape (n,)

        Examples:
            >>> service = EmbeddingService()
            >>> query = np.random.rand(384)
            >>> corpus = np.random.rand(100, 384)
            >>> scores = await service.compute_similarity_batch(query, corpus)
            >>> scores.shape
            (100,)
        """
        # Ensure inputs are numpy arrays
        query = np.asarray(query_embedding).reshape(1, -1)
        corpus = np.asarray(corpus_embeddings)

        # Normalize
        query_norm = query / (np.linalg.norm(query, axis=1, keepdims=True) + 1e-9)
        corpus_norm = corpus / (np.linalg.norm(corpus, axis=1, keepdims=True) + 1e-9)

        # Compute dot product (cosine similarity for normalized vectors)
        similarities = np.dot(corpus_norm, query_norm.T).flatten()

        return similarities

    def get_model_info(self) -> dict:
        """
        Get information about the current model.

        Returns:
            Dictionary with model configuration and metadata
        """
        return {
            "model_name": self.model_name,
            "dimension": self.dimension,
            "device": self.device or "auto",
            "max_seq_length": getattr(self.model, "max_seq_length", None),
            "is_loaded": self._model is not None,
        }


# Global singleton instance
_embedding_service: Optional[EmbeddingService] = None


def get_embedding_service(
    model_name: str = EmbeddingService.DEFAULT_MODEL,
    force_new: bool = False,
) -> EmbeddingService:
    """
    Get or create the global embedding service singleton.

    Args:
        model_name: Model name to use (only applies on first call)
        force_new: Force creation of new instance

    Returns:
        EmbeddingService instance

    Examples:
        >>> service = get_embedding_service()
        >>> embedding = await service.encode("Hello")
    """
    global _embedding_service

    if _embedding_service is None or force_new:
        _embedding_service = EmbeddingService(model_name=model_name)

    return _embedding_service
