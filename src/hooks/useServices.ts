// React Hooks 封装服务访问
// 这是渐进式重构的第二步，提供简单的hooks接口

import { audioService, fileService, voiceInputService } from '../services';

// 简单的hooks，直接返回服务实例
export const useAudioService = () => audioService;
export const useFileService = () => fileService;
export const useVoiceInputService = () => voiceInputService;
