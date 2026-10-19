import { get, post } from "submilli:http";
import { ProviderIssue, Result, YouTubeGetChannelOptions, YouTubeGetChannelData, YouTubeListChannelVideosOptions, YouTubeListChannelVideosData, YouTubeListChannelPlaylistsOptions, YouTubeListChannelPlaylistsData, YouTubeListChannelLivesOptions, YouTubeListChannelLivesData, YouTubeListChannelCommunityPostsOptions, YouTubeListChannelCommunityPostsData, YouTubeListChannelShortsOptions, YouTubeListChannelShortsData, YouTubeGetVideoOptions, YouTubeGetVideoData, YouTubeGetTranscriptOptions, YouTubeGetTranscriptData, YouTubeGetSponsorsOptions, YouTubeGetSponsorsData, YouTubeSearchOptions, YouTubeSearchData, YouTubeSearchTypeaheadOptions, YouTubeSearchTypeaheadData, YouTubeSearchHashtagOptions, YouTubeSearchHashtagData, YouTubeListCommentsOptions, YouTubeListCommentsData, YouTubeListCommentRepliesOptions, YouTubeListCommentRepliesData, YouTubeListTrendingShortsOptions, YouTubeListTrendingShortsData, YouTubeGetPlaylistOptions, YouTubeGetPlaylistData, YouTubeGetCommunityPostOptions, YouTubeGetCommunityPostData } from "./types";
export { ProviderIssue, Result, YouTubeGetChannelOptions, YouTubeGetChannelData, YouTubeListChannelVideosOptions, YouTubeListChannelVideosData, YouTubeListChannelPlaylistsOptions, YouTubeListChannelPlaylistsData, YouTubeListChannelLivesOptions, YouTubeListChannelLivesData, YouTubeListChannelCommunityPostsOptions, YouTubeListChannelCommunityPostsData, YouTubeListChannelShortsOptions, YouTubeListChannelShortsData, YouTubeGetVideoOptions, YouTubeGetVideoData, YouTubeGetTranscriptOptions, YouTubeGetTranscriptData, YouTubeGetSponsorsOptions, YouTubeGetSponsorsData, YouTubeSearchOptions, YouTubeSearchData, YouTubeSearchTypeaheadOptions, YouTubeSearchTypeaheadData, YouTubeSearchHashtagOptions, YouTubeSearchHashtagData, YouTubeListCommentsOptions, YouTubeListCommentsData, YouTubeListCommentRepliesOptions, YouTubeListCommentRepliesData, YouTubeListTrendingShortsOptions, YouTubeListTrendingShortsData, YouTubeGetPlaylistOptions, YouTubeGetPlaylistData, YouTubeGetCommunityPostOptions, YouTubeGetCommunityPostData } from "./types";
import { responseIssues, stringParam, booleanParam, integerParam, queryString, requireIdentity, authHeaders, readResponse, invalidArgument, invalidResponse, validateDates, validateSearchCursor, redditCommentsBody } from "./transport";
/** YouTube public-data operations through ScrapeCreators. */
export class YouTubeTransport {
    /** Channel Details.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    getChannel(options: YouTubeGetChannelOptions = {}): Result<YouTubeGetChannelData> {
        const params = new Map<string, string>();
        stringParam(params, "channelId", options.channelId, false);
        stringParam(params, "handle", options.handle, false);
        stringParam(params, "url", options.url, false);
        stringParam(params, "cache_max_age", options.cacheMaxAge, false, ["1d", "3d", "7d", "14d", "30d"]);
        requireIdentity([options.channelId, options.handle, options.url]);
        const response = get("https://api.scrapecreators.com/v1/youtube/channel" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as YouTubeGetChannelData;
            if (typeof data.channelId !== "string" || data.channelId.trim().length === 0)
                throw invalidResponse();
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponse();
        }
    }
    /** Channel Videos.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    listChannelVideos(options: YouTubeListChannelVideosOptions = {}): Result<YouTubeListChannelVideosData> {
        const params = new Map<string, string>();
        stringParam(params, "channelId", options.channelId, false);
        stringParam(params, "handle", options.handle, false);
        stringParam(params, "sort", options.sort, false, ["latest", "popular"]);
        stringParam(params, "continuationToken", options.continuationToken, false);
        booleanParam(params, "is_paid_promotions", options.isPaidPromotions, false);
        booleanParam(params, "includeExtras", options.includeExtras, false);
        requireIdentity([options.channelId, options.handle]);
        if (options.isPaidPromotions === true && options.includeExtras === true)
            throw invalidArgument("Paid promotion filtering cannot be combined with includeExtras");
        const response = get("https://api.scrapecreators.com/v1/youtube/channel-videos" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as YouTubeListChannelVideosData;
            if (!Array.isArray(data.videos))
                throw invalidResponse();
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponse();
        }
    }
    /** Channel Playlists.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    listChannelPlaylists(options: YouTubeListChannelPlaylistsOptions = {}): Result<YouTubeListChannelPlaylistsData> {
        const params = new Map<string, string>();
        stringParam(params, "channelId", options.channelId, false);
        stringParam(params, "handle", options.handle, false);
        stringParam(params, "continuationToken", options.continuationToken, false);
        requireIdentity([options.channelId, options.handle]);
        const response = get("https://api.scrapecreators.com/v1/youtube/channel/playlists" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as YouTubeListChannelPlaylistsData;
            if (!Array.isArray(data.playlists))
                throw invalidResponse();
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponse();
        }
    }
    /** Channel Lives.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    listChannelLives(options: YouTubeListChannelLivesOptions = {}): Result<YouTubeListChannelLivesData> {
        const params = new Map<string, string>();
        stringParam(params, "channelId", options.channelId, false);
        stringParam(params, "handle", options.handle, false);
        stringParam(params, "continuationToken", options.continuationToken, false);
        requireIdentity([options.channelId, options.handle]);
        const response = get("https://api.scrapecreators.com/v1/youtube/channel/lives" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as YouTubeListChannelLivesData;
            if (!Array.isArray(data.lives))
                throw invalidResponse();
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponse();
        }
    }
    /** Channel Community Posts.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    listChannelCommunityPosts(options: YouTubeListChannelCommunityPostsOptions = {}): Result<YouTubeListChannelCommunityPostsData> {
        const params = new Map<string, string>();
        stringParam(params, "channelId", options.channelId, false);
        stringParam(params, "handle", options.handle, false);
        stringParam(params, "continuationToken", options.continuationToken, false);
        requireIdentity([options.channelId, options.handle]);
        const response = get("https://api.scrapecreators.com/v1/youtube/channel/community-posts" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as YouTubeListChannelCommunityPostsData;
            if (!Array.isArray(data.posts))
                throw invalidResponse();
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponse();
        }
    }
    /** Channel Shorts.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    listChannelShorts(options: YouTubeListChannelShortsOptions = {}): Result<YouTubeListChannelShortsData> {
        const params = new Map<string, string>();
        stringParam(params, "handle", options.handle, false);
        stringParam(params, "channelId", options.channelId, false);
        stringParam(params, "sort", options.sort, false, ["newest", "popular"]);
        stringParam(params, "continuationToken", options.continuationToken, false);
        requireIdentity([options.handle, options.channelId]);
        const response = get("https://api.scrapecreators.com/v1/youtube/channel/shorts" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as YouTubeListChannelShortsData;
            if (!Array.isArray(data.shorts))
                throw invalidResponse();
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponse();
        }
    }
    /** Video/Short Details.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    getVideo(options: YouTubeGetVideoOptions): Result<YouTubeGetVideoData> {
        const params = new Map<string, string>();
        stringParam(params, "url", options.url, true);
        stringParam(params, "language", options.language, false);
        stringParam(params, "cache_max_age", options.cacheMaxAge, false, ["1d", "3d", "7d", "14d", "30d"]);
        const response = get("https://api.scrapecreators.com/v1/youtube/video" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as YouTubeGetVideoData;
            if (typeof data.id !== "string" || data.id.trim().length === 0)
                throw invalidResponse();
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponse();
        }
    }
    /** Transcript.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    getTranscript(options: YouTubeGetTranscriptOptions): Result<YouTubeGetTranscriptData> {
        const params = new Map<string, string>();
        stringParam(params, "url", options.url, true);
        stringParam(params, "language", options.language, false);
        booleanParam(params, "original_audio", options.originalAudio, false);
        stringParam(params, "cache_max_age", options.cacheMaxAge, false, ["1d", "3d", "7d", "14d", "30d"]);
        const response = get("https://api.scrapecreators.com/v1/youtube/video/transcript" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as YouTubeGetTranscriptData;
            if (data.transcript !== null && !Array.isArray(data.transcript))
                throw invalidResponse();
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponse();
        }
    }
    /** Video Sponsors.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    getSponsors(options: YouTubeGetSponsorsOptions): Result<YouTubeGetSponsorsData> {
        const params = new Map<string, string>();
        stringParam(params, "url", options.url, true);
        stringParam(params, "language", options.language, false);
        const response = get("https://api.scrapecreators.com/v1/youtube/video/sponsors" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as YouTubeGetSponsorsData;
            if (data.video === null || typeof data.video !== "object" || Array.isArray(data.video))
                throw invalidResponse();
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponse();
        }
    }
    /** Search.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    search(options: YouTubeSearchOptions): Result<YouTubeSearchData> {
        const params = new Map<string, string>();
        stringParam(params, "query", options.query, true);
        stringParam(params, "uploadDate", options.uploadDate, false, ["today", "this_week", "this_month", "this_year"]);
        stringParam(params, "sortBy", options.sortBy, false, ["relevance", "popular"]);
        stringParam(params, "type", options.type, false, ["videos", "shorts", "channels", "playlists"]);
        stringParam(params, "duration", options.duration, false, ["under_3_min", "between_3_and_20_min", "over_20_min"]);
        stringParam(params, "region", options.region, false);
        stringParam(params, "continuationToken", options.continuationToken, false);
        booleanParam(params, "includeExtras", options.includeExtras, false);
        const response = get("https://api.scrapecreators.com/v1/youtube/search" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as YouTubeSearchData;
            if (!Array.isArray(data.videos))
                throw invalidResponse();
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponse();
        }
    }
    /** Search Typeahead.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    searchTypeahead(options: YouTubeSearchTypeaheadOptions): Result<YouTubeSearchTypeaheadData> {
        const params = new Map<string, string>();
        stringParam(params, "query", options.query, true);
        const response = get("https://api.scrapecreators.com/v1/youtube/search/typeahead" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as YouTubeSearchTypeaheadData;
            if (data.data === null || typeof data.data !== "object" || Array.isArray(data.data))
                throw invalidResponse();
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponse();
        }
    }
    /** Search by Hashtag.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    searchHashtag(options: YouTubeSearchHashtagOptions): Result<YouTubeSearchHashtagData> {
        const params = new Map<string, string>();
        stringParam(params, "hashtag", options.hashtag, true);
        stringParam(params, "continuationToken", options.continuationToken, false);
        stringParam(params, "type", options.type, false, ["all", "shorts"]);
        const response = get("https://api.scrapecreators.com/v1/youtube/search/hashtag" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as YouTubeSearchHashtagData;
            if (!Array.isArray(data.videos))
                throw invalidResponse();
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponse();
        }
    }
    /** Comments.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    listComments(options: YouTubeListCommentsOptions): Result<YouTubeListCommentsData> {
        const params = new Map<string, string>();
        stringParam(params, "url", options.url, true);
        stringParam(params, "continuationToken", options.continuationToken, false);
        stringParam(params, "order", options.order, false, ["top", "newest"]);
        const response = get("https://api.scrapecreators.com/v1/youtube/video/comments" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as YouTubeListCommentsData;
            if (!Array.isArray(data.comments))
                throw invalidResponse();
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponse();
        }
    }
    /** Comment Replies.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    listCommentReplies(options: YouTubeListCommentRepliesOptions): Result<YouTubeListCommentRepliesData> {
        const params = new Map<string, string>();
        stringParam(params, "continuationToken", options.continuationToken, true);
        const response = get("https://api.scrapecreators.com/v1/youtube/video/comment/replies" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as YouTubeListCommentRepliesData;
            if (!Array.isArray(data.comments))
                throw invalidResponse();
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponse();
        }
    }
    /** Trending Shorts.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    listTrendingShorts(options: YouTubeListTrendingShortsOptions = {}): Result<YouTubeListTrendingShortsData> {
        const params = new Map<string, string>();
        const response = get("https://api.scrapecreators.com/v1/youtube/shorts/trending" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as YouTubeListTrendingShortsData;
            if (!Array.isArray(data.shorts))
                throw invalidResponse();
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponse();
        }
    }
    /** Playlist.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    getPlaylist(options: YouTubeGetPlaylistOptions): Result<YouTubeGetPlaylistData> {
        const params = new Map<string, string>();
        stringParam(params, "playlist_id", options.playlistId, true);
        stringParam(params, "continuationToken", options.continuationToken, false);
        const response = get("https://api.scrapecreators.com/v1/youtube/playlist" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as YouTubeGetPlaylistData;
            if (!Array.isArray(data.videos))
                throw invalidResponse();
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponse();
        }
    }
    /** Community Post Details.
     * @param options Provider controls; omit optional values to retain provider defaults.
     * @returns One typed response and its complete raw JSON.
     */
    getCommunityPost(options: YouTubeGetCommunityPostOptions): Result<YouTubeGetCommunityPostData> {
        const params = new Map<string, string>();
        stringParam(params, "url", options.url, true);
        const response = get("https://api.scrapecreators.com/v1/youtube/community-post" + queryString(params), authHeaders());
        const raw = readResponse(response);
        try {
            const data = raw as YouTubeGetCommunityPostData;
            if (typeof data.id !== "string" || data.id.trim().length === 0)
                throw invalidResponse();
            return { data: data, raw: raw, issues: responseIssues(raw) };
        }
        catch (cause) {
            throw invalidResponse();
        }
    }
}
