/**
 * ECharts 共享配置
 *
 * 统一注册所有图表组件，避免在各个组件中重复注册
 */
import { LineChart } from 'echarts/charts'
import {
  AriaComponent,
  DataZoomComponent,
  GridComponent,
  LegendComponent,
  MarkLineComponent,
  MarkPointComponent,
  TooltipComponent,
} from 'echarts/components'
import { use } from 'echarts/core'
import { CanvasRenderer } from 'echarts/renderers'

// 一次性注册所有需要的 ECharts 组件
use([
  AriaComponent,
  DataZoomComponent,
  LineChart,
  GridComponent,
  TooltipComponent,
  LegendComponent,
  MarkLineComponent,
  MarkPointComponent,
  CanvasRenderer,
])
