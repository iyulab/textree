namespace Textree.Host.Contracts;

public sealed record ChatRequestDto(List<ChatMessageDto> Messages, bool Stream = true, int? MaxTokens = null);
public sealed record ChatMessageDto(string Role, string Content);
